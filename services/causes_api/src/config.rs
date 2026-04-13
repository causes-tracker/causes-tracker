use clap::Parser;

/// Runtime configuration, read entirely from environment variables
/// (with `.env` file support via dotenvy).
#[derive(Parser, Debug, Clone)]
#[command(about = "Causes API server")]
pub struct Config {
    /// PostgreSQL connection string (static-password mode).
    /// Required when IAM auth fields (DB_HOST, DB_USER) are not set.
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: Option<String>,

    /// RDS hostname for IAM-authenticated connections.
    /// When set together with `db_user`, enables IAM auth mode and
    /// `database_url` is ignored.
    #[arg(long, env = "DB_HOST")]
    pub db_host: Option<String>,

    /// Database username for IAM-authenticated connections.
    #[arg(long, env = "DB_USER")]
    pub db_user: Option<String>,

    /// Database port (defaults to 5432).
    #[arg(long, env = "DB_PORT", default_value_t = 5432)]
    pub db_port: u16,

    /// Google OAuth 2.0 Client ID (TV and Limited Input devices type).
    /// Required during first-time bootstrap; can be unset after an admin
    /// has been created.
    #[arg(long, env = "GOOGLE_CLIENT_ID", default_value = "")]
    pub google_client_id: String,

    /// Google OAuth 2.0 Client Secret paired with `GOOGLE_CLIENT_ID`.
    #[arg(long, env = "GOOGLE_CLIENT_SECRET", default_value = "")]
    pub google_client_secret: String,

    /// Honeycomb API key for OpenTelemetry OTLP export.
    /// When absent, traces are not exported and only structured JSON logs
    /// are written to stdout.
    #[arg(long, env = "HONEYCOMB_API_KEY")]
    pub honeycomb_api_key: Option<String>,

    /// Honeycomb OTLP endpoint.
    /// Use https://api.eu1.honeycomb.io:443 for the EU partition.
    #[arg(
        long,
        env = "HONEYCOMB_ENDPOINT",
        default_value = "https://api.honeycomb.io:443"
    )]
    pub honeycomb_endpoint: String,

    /// gRPC listen address (used when TLS_DOMAIN is not set).
    #[arg(long, env = "BIND_ADDR", default_value = "[::]:50051")]
    pub bind_addr: String,

    /// Domain for automatic TLS via Let's Encrypt (e.g. "causes.example.com").
    /// When set, the server listens on port 443 with auto-renewing certificates.
    /// When unset, the server runs plain HTTP/2 on BIND_ADDR (dev mode).
    #[arg(long, env = "TLS_DOMAIN")]
    pub tls_domain: Option<String>,

    /// ACME contact email for Let's Encrypt certificate notifications.
    #[arg(long, env = "TLS_ACME_EMAIL")]
    pub tls_acme_email: Option<String>,

    /// Directory to cache TLS certificates.  Must persist across restarts.
    #[arg(
        long,
        env = "TLS_CERT_CACHE_DIR",
        default_value = "/var/lib/causes/certs"
    )]
    pub tls_cert_cache_dir: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bind_addr() {
        let cfg = Config::parse_from(["causes_api", "--database-url=postgresql://test"]);
        assert_eq!(cfg.bind_addr, "[::]:50051");
        assert!(cfg.honeycomb_api_key.is_none());
        assert!(cfg.tls_domain.is_none());
        assert!(cfg.tls_acme_email.is_none());
        assert_eq!(cfg.tls_cert_cache_dir, "/var/lib/causes/certs");
        assert_eq!(cfg.db_port, 5432);
        assert!(cfg.db_host.is_none());
        assert!(cfg.db_user.is_none());
    }

    #[test]
    fn bind_addr_override() {
        let cfg = Config::parse_from([
            "causes_api",
            "--database-url=postgresql://test",
            "--bind-addr=[::]:9090",
        ]);
        assert_eq!(cfg.bind_addr, "[::]:9090");
    }

    #[test]
    fn iam_mode_fields() {
        let cfg = Config::parse_from([
            "causes_api",
            "--db-host=mydb.rds.amazonaws.com",
            "--db-user=causes",
            "--db-port=5433",
        ]);
        assert_eq!(cfg.db_host.as_deref(), Some("mydb.rds.amazonaws.com"));
        assert_eq!(cfg.db_user.as_deref(), Some("causes"));
        assert_eq!(cfg.db_port, 5433);
        assert!(cfg.database_url.is_none());
    }
}
