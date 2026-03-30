# infra/terraform

Provisions the Causes production infrastructure in AWS: Aurora PostgreSQL, EC2 (causes-api container), S3 (image storage), and supporting networking/IAM.
Idle cost is ~$5/month in ap-southeast-2.

The causes-tracker project itself runs on this infrastructure at `https://causes-tracker.robertcollins.net`.

All commands below use the hermetic OpenTofu wrapper:
`bazel run //infra:tofu -- <args>`.

## First-time provisioning

### 1. Ensure IAM permissions

See [IAM permissions](#iam-permissions) below.

### 2. Generate an SSH key

```sh
ssh-keygen -t ed25519 -f infra/terraform/causes-deployer
```

Both files are gitignored.

### 3. Configure variables

Edit `terraform.tfvars` (gitignored):

```sh
echo "ssh_public_key = \"$(cat infra/terraform/causes-deployer.pub)\"" >> infra/terraform/terraform.tfvars
```

See [Variables](#variables) for the full list.
At minimum you need `region`, `ssh_public_key`, `google_client_id`, and `google_client_secret`.

### 4. Create the S3 bucket

The EC2 instance pulls its container image from S3 on first boot, so the bucket and image must exist before the instance is created.

```sh
bazel run //infra:tofu -- init
bazel run //infra:tofu -- apply \
  -target=aws_s3_bucket.images \
  -target=aws_s3_bucket_versioning.images \
  -target=aws_s3_bucket_public_access_block.images
```

### 5. Build and upload the container image

```sh
bazel build //services/causes_api:image_tarball
BUCKET=$(bazel --quiet run //infra:tofu -- output -raw images_bucket)
TARBALL="$(bazel info workspace)/bazel-bin/services/causes_api/image_load/tarball.tar"
bazel run //infra:aws -- s3 cp "$TARBALL" s3://$BUCKET/causes-api-latest.tar
```

### 6. Provision everything else

```sh
bazel run //infra:tofu -- apply
```

This creates the VPC, Aurora cluster, EC2 instance, EIP, and IAM roles.
The EC2 `user_data` script pulls the image from S3 and starts the container.

### 7. Enable pgvector

pgvector is built into Aurora PostgreSQL.
Once you have connectivity to the cluster from inside the VPC (e.g. SSH to EC2), run:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

### 8. Verify

```sh
IP=$(bazel --quiet run //infra:tofu -- output -raw ec2_public_ip)
ssh -i infra/terraform/causes-deployer ec2-user@$IP
docker logs causes-api
```

The bootstrap flow prints a Google device code to stdout.
Complete the flow and verify the admin was created.

## Deploy a new image

```sh
bazel run //infra:deploy
```

This builds the OCI image, uploads it to S3, and replaces the EC2 instance in one step.

## Update environment variables

After changing values in `terraform.tfvars`, the `user_data` script is updated but the running instance is not affected.
Force the instance to be recreated:

```sh
bazel run //infra:tofu -- apply -replace=aws_instance.causes_api
IP=$(bazel --quiet run //infra:tofu -- output -raw ec2_public_ip)
ssh-keygen -R $IP
```

This destroys and recreates the EC2 instance, re-running `user_data` with the new values.
The EIP is preserved.

## Tear down

```sh
bazel run //infra:tofu -- destroy
```

## Variables

Set in `terraform.tfvars` (gitignored).

| Variable | Required | Default | Description |
|---|---|---|---|
| `region` | yes | — | AWS region (e.g. `ap-southeast-2`) |
| `ssh_public_key` | yes | — | SSH public key for EC2 access |
| `ssh_allow_cidr` | yes | — | CIDR allowed to SSH to EC2 (e.g. `203.0.113.1/32`) |
| `google_client_id` | yes | — | Google OAuth 2.0 Client ID (TV/Limited Input type) |
| `google_client_secret` | yes | — | Google OAuth 2.0 Client Secret |
| `honeycomb_api_key` | no | `""` | Honeycomb API key; when empty, tracing is disabled |
| `honeycomb_endpoint` | no | `https://api.honeycomb.io:443` | OTLP endpoint; use `https://api.eu1.honeycomb.io:443` for EU |
| `tls_domain` | no | `""` | Domain for automatic TLS (e.g. `causes.example.com`); when empty, TLS is disabled |
| `tls_acme_email` | no | `""` | Contact email for Let's Encrypt certificate notifications |

## Enabling TLS

To serve gRPC over TLS with automatic Let's Encrypt certificates:

1. Create a DNS A record pointing your domain at the Elastic IP:

   ```sh
   bazel --quiet run //infra:tofu -- output -raw ec2_public_ip
   ```

2. Add to `terraform.tfvars`:

   ```hcl
   tls_domain     = "causes.example.com"
   tls_acme_email = "admin@example.com"
   ```

3. Apply and recreate the instance:

   ```sh
   bazel run //infra:tofu -- apply -replace=aws_instance.causes_api
   ```

The server obtains a certificate on first start (~30 seconds) and auto-renews before expiry.
Certificates are cached in a Docker volume at `/var/lib/causes/certs` so they survive container restarts.

## IAM permissions for the OpenTofu operator

These are the AWS permissions your own account (or CI role) needs to run `tofu plan`, `tofu apply`, and `tofu destroy`.
They are separate from the IAM roles that tofu creates for the EC2 instance.

### Plan, apply, and destroy

Attach these read-only managed policies:

| AWS managed policy | Why |
|---|---|
| `AmazonVPCReadOnlyAccess` | Read AZs, VPCs, subnets, security groups |
| `AmazonRDSReadOnlyAccess` | Read DB subnet groups, clusters, instances |
| `AmazonEC2ReadOnlyAccess` | Read instances, EIPs, key pairs, EBS volumes |
| `AmazonS3ReadOnlyAccess` | Read S3 buckets |
| `IAMReadOnlyAccess` | Read IAM roles, policies, instance profiles |

For apply and destroy, attach this inline policy:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "Vpc",
      "Effect": "Allow",
      "Action": [
        "ec2:CreateVpc", "ec2:DeleteVpc", "ec2:ModifyVpcAttribute",
        "ec2:CreateSubnet", "ec2:DeleteSubnet",
        "ec2:CreateInternetGateway", "ec2:DeleteInternetGateway",
        "ec2:AttachInternetGateway", "ec2:DetachInternetGateway",
        "ec2:CreateRouteTable", "ec2:DeleteRouteTable",
        "ec2:CreateRoute", "ec2:DeleteRoute",
        "ec2:AssociateRouteTable", "ec2:DisassociateRouteTable",
        "ec2:CreateSecurityGroup", "ec2:DeleteSecurityGroup",
        "ec2:AuthorizeSecurityGroupIngress", "ec2:RevokeSecurityGroupIngress",
        "ec2:AuthorizeSecurityGroupEgress", "ec2:RevokeSecurityGroupEgress",
        "ec2:CreateTags"
      ],
      "Resource": "*"
    },
    {
      "Sid": "Ec2",
      "Effect": "Allow",
      "Action": [
        "ec2:ImportKeyPair", "ec2:DeleteKeyPair",
        "ec2:RunInstances", "ec2:TerminateInstances",
        "ec2:StopInstances", "ec2:StartInstances",
        "ec2:ModifyInstanceAttribute",
        "ec2:AllocateAddress", "ec2:ReleaseAddress",
        "ec2:AssociateAddress", "ec2:DisassociateAddress",
        "ec2:CreateVolume", "ec2:DeleteVolume",
        "ec2:AttachVolume", "ec2:DetachVolume"
      ],
      "Resource": "*"
    },
    {
      "Sid": "Rds",
      "Effect": "Allow",
      "Action": [
        "rds:CreateDBSubnetGroup", "rds:DeleteDBSubnetGroup",
        "rds:ModifyDBSubnetGroup",
        "rds:CreateDBCluster", "rds:DeleteDBCluster",
        "rds:ModifyDBCluster",
        "rds:CreateDBInstance", "rds:DeleteDBInstance",
        "rds:AddTagsToResource", "rds:RemoveTagsFromResource"
      ],
      "Resource": "*"
    },
    {
      "Sid": "RdsManagedSecret",
      "Effect": "Allow",
      "Action": [
        "secretsmanager:CreateSecret",
        "secretsmanager:TagResource",
        "secretsmanager:DescribeSecret",
        "secretsmanager:DeleteSecret"
      ],
      "Resource": "arn:aws:secretsmanager:*:*:secret:rds!cluster-*"
    },
    {
      "Sid": "RdsServiceLinkedRole",
      "Effect": "Allow",
      "Action": "iam:CreateServiceLinkedRole",
      "Resource": "arn:aws:iam::*:role/aws-service-role/rds.amazonaws.com/*",
      "Condition": {
        "StringEquals": { "iam:AWSServiceName": "rds.amazonaws.com" }
      }
    },
    {
      "Sid": "S3",
      "Effect": "Allow",
      "Action": [
        "s3:CreateBucket", "s3:DeleteBucket",
        "s3:PutBucketVersioning", "s3:GetBucketVersioning",
        "s3:PutBucketPublicAccessBlock", "s3:GetBucketPublicAccessBlock",
        "s3:PutBucketTagging", "s3:GetBucketTagging",
        "s3:PutObject", "s3:GetObject", "s3:DeleteObject"
      ],
      "Resource": ["arn:aws:s3:::causes-images-*", "arn:aws:s3:::causes-images-*/*"]
    },
    {
      "Sid": "Iam",
      "Effect": "Allow",
      "Action": [
        "iam:CreateRole", "iam:DeleteRole", "iam:GetRole",
        "iam:PutRolePolicy", "iam:DeleteRolePolicy", "iam:GetRolePolicy",
        "iam:ListRolePolicies", "iam:ListAttachedRolePolicies",
        "iam:ListInstanceProfilesForRole",
        "iam:CreateInstanceProfile", "iam:DeleteInstanceProfile",
        "iam:GetInstanceProfile",
        "iam:AddRoleToInstanceProfile", "iam:RemoveRoleFromInstanceProfile",
        "iam:PassRole", "iam:TagRole"
      ],
      "Resource": [
        "arn:aws:iam::*:role/causes-*",
        "arn:aws:iam::*:instance-profile/causes-*"
      ]
    },
    {
      "Sid": "SsmAmiLookup",
      "Effect": "Allow",
      "Action": "ssm:GetParameter",
      "Resource": "arn:aws:ssm:*::parameter/aws/service/ami-amazon-linux-latest/*"
    }
  ]
}
```

If no Aurora cluster has ever been created in the AWS account, the RDS service-linked role (`AWSServiceRoleForRDS`) may not exist yet.
The `RdsServiceLinkedRole` statement above creates it on first `tofu apply`.
