output "writer_endpoint" {
  description = "Aurora cluster writer endpoint."
  value       = aws_rds_cluster.causes.endpoint
}

output "port" {
  description = "Database port."
  value       = aws_rds_cluster.causes.port
}

output "secret_arn" {
  description = "ARN of the Secrets Manager secret holding the master password."
  value       = aws_rds_cluster.causes.master_user_secret[0].secret_arn
}

output "ec2_public_ip" {
  description = "Elastic IP of the causes-api EC2 instance."
  value       = aws_eip.causes_api.public_ip
}

output "images_bucket" {
  description = "S3 bucket name for container image tarballs."
  value       = aws_s3_bucket.images.id
}
