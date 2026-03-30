variable "region" {
  description = "AWS region to deploy into."
  type        = string
}

variable "ssh_public_key" {
  description = "SSH public key for EC2 access."
  type        = string
}

variable "google_client_id" {
  description = "Google OAuth 2.0 Client ID (TV/Limited Input type)."
  type        = string
}

variable "google_client_secret" {
  description = "Google OAuth 2.0 Client Secret."
  type        = string
  sensitive   = true
}

variable "ssh_allow_cidr" {
  description = "CIDR block allowed to SSH to the EC2 instance (e.g. your IP: 203.0.113.1/32)."
  type        = string
}

variable "honeycomb_api_key" {
  description = "Honeycomb API key for OpenTelemetry OTLP export. When empty, tracing is disabled."
  type        = string
  default     = ""
  sensitive   = true
}

variable "honeycomb_endpoint" {
  description = "Honeycomb OTLP endpoint. Use https://api.eu1.honeycomb.io:443 for EU."
  type        = string
  default     = "https://api.honeycomb.io:443"
}

variable "tls_domain" {
  description = "Domain for automatic TLS via Let's Encrypt. When empty, TLS is disabled and the server listens on port 50051 (plain HTTP/2)."
  type        = string
  default     = ""
}

variable "tls_acme_email" {
  description = "Contact email for Let's Encrypt certificate notifications."
  type        = string
  default     = ""
}
