#!/usr/bin/env bash
# Build, upload, and deploy the causes-api container image.
#
# Usage: bazel run //infra:deploy
#
# Steps:
#   1. Build the OCI image tarball
#   2. Upload it to the S3 images bucket
#   3. Replace the EC2 instance to pick up the new image
set -euo pipefail

cd "${BUILD_WORKSPACE_DIRECTORY}"

echo "==> Building image tarball"
bazel --quiet build //services/causes_api:image_tarball

BUCKET=$(bazel --quiet run //infra:tofu -- infra/terraform output -raw images_bucket)
TARBALL="${BUILD_WORKSPACE_DIRECTORY}/bazel-bin/services/causes_api/image_load/tarball.tar"

echo "==> Uploading to s3://${BUCKET}/causes-api-latest.tar"
bazel --quiet run //infra:aws -- s3 cp "$TARBALL" "s3://${BUCKET}/causes-api-latest.tar"

echo "==> Replacing EC2 instance"
bazel --quiet run //infra:tofu -- infra/terraform apply -replace=aws_instance.causes_api -auto-approve

IP=$(bazel --quiet run //infra:tofu -- infra/terraform output -raw ec2_public_ip)
ssh-keygen -R "$IP" 2>/dev/null || true

echo "==> Deployed. Instance at ${IP}"
echo "    Wait ~30s for the container to start and obtain a TLS certificate."
