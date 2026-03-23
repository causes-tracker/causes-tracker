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
