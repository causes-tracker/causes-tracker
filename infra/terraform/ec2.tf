# ── EC2 t3.nano (x86_64) ─────────────────────────────────────────────────────
#
# Runs the causes-api container with direct TLS termination via rustls-acme.
# ~$4.70/month in ap-southeast-2.

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

resource "aws_instance" "causes_api" {
  ami                         = data.aws_ssm_parameter.al2023_x86_64.value
  instance_type               = "t3.nano"
  subnet_id                   = aws_subnet.causes_a.id
  vpc_security_group_ids      = [aws_security_group.causes_api.id, aws_security_group.causes_db.id]
  key_name                    = aws_key_pair.deployer.key_name
  associate_public_ip_address = true

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

    # Pull the DB password from Secrets Manager and build DATABASE_URL.
    # The RDS-managed password may contain shell-special and URI-special
    # characters, so we URL-encode it via jq @uri.
    DB_SECRET=$(aws secretsmanager get-secret-value \
      --secret-id "${aws_rds_cluster.causes.master_user_secret[0].secret_arn}" \
      --query SecretString --output text)
    DB_USER=$(echo "$DB_SECRET" | jq -r .username)
    DB_PASS=$(echo "$DB_SECRET" | jq -r '.password | @uri')
    DB_HOST="${aws_rds_cluster.causes.endpoint}"
    DB_PORT="${aws_rds_cluster.causes.port}"
    DATABASE_URL="postgresql://$DB_USER:$DB_PASS@$DB_HOST:$DB_PORT/causes"

    # Cert cache directory (persists across container restarts).
    mkdir -p /var/lib/causes/certs

    # Allow binding port 443 without root.
    sysctl -w net.ipv4.ip_unprivileged_port_start=0

    # Start causes-api
    docker run -d --name causes-api --restart unless-stopped \
      -p 443:443 \
      -v /var/lib/causes/certs:/var/lib/causes/certs \
      -e DATABASE_URL="$DATABASE_URL" \
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

# ── IAM role for Secrets Manager + S3 access ─────────────────────────────────

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