# ── EC2 t3.nano (x86_64) ─────────────────────────────────────────────────────
#
# Runs the causes-api container with direct TLS termination via rustls-acme.
# ~$4.70/month in ap-southeast-2.

data "aws_caller_identity" "current" {}

data "aws_ssm_parameter" "al2023_x86_64" {
  name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64"
}

resource "aws_internet_gateway" "causes" {
  vpc_id = aws_vpc.causes.id
  tags   = { Name = "causes" }
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.causes.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.causes.id
  }

  tags = { Name = "causes-public" }
}

resource "aws_route_table_association" "causes_a" {
  subnet_id      = aws_subnet.causes_a.id
  route_table_id = aws_route_table.public.id
}

resource "aws_security_group" "causes_api" {
  name   = "causes-api"
  vpc_id = aws_vpc.causes.id

  ingress {
    description = "SSH"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.ssh_allow_cidr]
  }

  ingress {
    description = "HTTPS / gRPC (TLS-ALPN-01 + h2)"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = { Name = "causes-api" }
}

resource "aws_key_pair" "deployer" {
  key_name   = "causes-deployer"
  public_key = var.ssh_public_key
}

# ── S3 bucket for container images ───────────────────────────────────────────
#
# Deploy workflow:
#   bazel build //services/causes_api:image_tarball
#   BUCKET=$(bazel --quiet run //infra:tofu -- output -raw images_bucket)
#   bazel run //infra:aws -- s3 cp \
#     bazel-bin/services/causes_api/image_load/tarball.tar \
#     s3://$BUCKET/causes-api-latest.tar
#   # Then restart/recreate the EC2 instance, or SSH in and re-pull.

resource "random_id" "images_suffix" {
  byte_length = 4
}

resource "aws_s3_bucket" "images" {
  bucket = "causes-images-${random_id.images_suffix.hex}"
  tags   = { Name = "causes-images" }
}

resource "aws_s3_bucket_versioning" "images" {
  bucket = aws_s3_bucket.images.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_public_access_block" "images" {
  bucket                  = aws_s3_bucket.images.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# ── Persistent EBS volume for TLS certificate cache ──────────────────────────
#
# Survives instance replacement (tofu apply -replace=aws_instance.causes_api)
# so Let's Encrypt certs are reused.  Without this, a fresh cert would be
# issued on every deploy — LE rate-limits to 5 per domain per week.

resource "aws_ebs_volume" "certs" {
  availability_zone = data.aws_availability_zones.available.names[0]
  size              = 1 # GiB — certs are tiny
  type              = "gp3"
  tags              = { Name = "causes-certs" }
}

resource "aws_volume_attachment" "certs" {
  device_name = "/dev/xvdf"
  volume_id   = aws_ebs_volume.certs.id
  instance_id = aws_instance.causes_api.id
}

resource "aws_instance" "causes_api" {
  ami                         = data.aws_ssm_parameter.al2023_x86_64.value
  instance_type               = "t3.nano"
  subnet_id                   = aws_subnet.causes_a.id
  vpc_security_group_ids      = [aws_security_group.causes_api.id, aws_security_group.causes_db.id]
  key_name                    = aws_key_pair.deployer.key_name
  associate_public_ip_address = true

  # IMDSv2 defaults to hop_limit=1, which blocks IMDS from Docker bridge
  # containers (the bridge adds one hop, so TTL reaches 0).  Bump to 2
  # so the AWS SDK inside the container can obtain IAM credentials.
  metadata_options {
    http_tokens                 = "required"
    http_put_response_hop_limit = 2
  }

  user_data = <<-USERDATA
    #!/bin/bash
    set -euo pipefail

    # Install Docker
    dnf install -y docker
    systemctl enable --now docker
    usermod -aG docker ec2-user

    # Pull the container image from S3
    aws s3 cp s3://${aws_s3_bucket.images.id}/causes-api-latest.tar /tmp/causes-api.tar
    docker load < /tmp/causes-api.tar
    rm /tmp/causes-api.tar

    # Mount the persistent EBS volume for TLS cert cache.
    # Format only if not already formatted (first attach).
    while [ ! -e /dev/xvdf ]; do sleep 1; done
    if ! blkid /dev/xvdf; then
      mkfs.ext4 /dev/xvdf
    fi
    mkdir -p /var/lib/causes/certs
    mount /dev/xvdf /var/lib/causes/certs
    echo '/dev/xvdf /var/lib/causes/certs ext4 defaults,nofail 0 2' >> /etc/fstab

    # Create the application database user with IAM authentication.
    # The master user (postgres) cannot use IAM auth, so we create a
    # separate "causes" user with rds_iam and full privileges on the
    # causes database.  Idempotent: the IF NOT EXISTS / GRANT are safe
    # to re-run.
    dnf install -y postgresql16
    DB_SECRET=$(aws secretsmanager get-secret-value \
      --secret-id "${aws_rds_cluster.causes.master_user_secret[0].secret_arn}" \
      --query SecretString --output text)
    DB_PASS=$(echo "$DB_SECRET" | jq -r .password)
    PGPASSWORD="$DB_PASS" psql \
      -h "${aws_rds_cluster.causes.endpoint}" \
      -p "${aws_rds_cluster.causes.port}" \
      -U "${aws_rds_cluster.causes.master_username}" \
      -d causes \
      -c "DO \$\$
          BEGIN
            IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'causes') THEN
              CREATE USER causes;
            END IF;
          END \$\$;" \
      -c "GRANT rds_iam TO causes" \
      -c "GRANT ALL ON DATABASE causes TO causes" \
      -c "GRANT ALL ON SCHEMA public TO causes" \
      -c "ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO causes" \
      -c "ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON SEQUENCES TO causes"

    # Allow binding port 443 without root.
    sysctl -w net.ipv4.ip_unprivileged_port_start=0

    # Start causes-api with IAM database authentication.
    # The container discovers IAM credentials via the EC2 Instance
    # Metadata Service (IMDS) — no secrets are passed as env vars.
    docker run -d --name causes-api --restart unless-stopped \
      -p 443:443 \
      -v /var/lib/causes/certs:/var/lib/causes/certs \
      -e DB_HOST="${aws_rds_cluster.causes.endpoint}" \
      -e DB_PORT="${aws_rds_cluster.causes.port}" \
      -e DB_USER="causes" \
      -e AWS_DEFAULT_REGION="${var.region}" \
      -e GOOGLE_CLIENT_ID="${var.google_client_id}" \
      -e GOOGLE_CLIENT_SECRET="${var.google_client_secret}" \
      -e HONEYCOMB_API_KEY="${var.honeycomb_api_key}" \
      -e HONEYCOMB_ENDPOINT="${var.honeycomb_endpoint}" \
      -e TLS_DOMAIN="${var.tls_domain}" \
      -e TLS_ACME_EMAIL="${var.tls_acme_email}" \
      -e TLS_CERT_CACHE_DIR="/var/lib/causes/certs" \
      causes-api:latest
  USERDATA

  iam_instance_profile = aws_iam_instance_profile.causes_api.name

  tags = { Name = "causes-api" }
}

resource "aws_eip" "causes_api" {
  instance = aws_instance.causes_api.id
  tags     = { Name = "causes-api" }
}

# ── IAM role for RDS IAM auth + S3 access ────────────────────────────────────

resource "aws_iam_role" "causes_api" {
  name = "causes-api"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy" "causes_api_secrets" {
  name = "causes-api-secrets"
  role = aws_iam_role.causes_api.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = [aws_rds_cluster.causes.master_user_secret[0].secret_arn]
    }]
  })
}

resource "aws_iam_role_policy" "causes_api_rds_iam" {
  name = "causes-api-rds-iam"
  role = aws_iam_role.causes_api.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = ["rds-db:connect"]
      Resource = [
        "arn:aws:rds-db:${var.region}:${data.aws_caller_identity.current.account_id}:dbuser:${aws_rds_cluster.causes.cluster_resource_id}/causes"
      ]
    }]
  })
}

resource "aws_iam_role_policy" "causes_api_s3" {
  name = "causes-api-s3"
  role = aws_iam_role.causes_api.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["s3:GetObject"]
      Resource = ["${aws_s3_bucket.images.arn}/*"]
    }]
  })
}

resource "aws_iam_instance_profile" "causes_api" {
  name = "causes-api"
  role = aws_iam_role.causes_api.name
}