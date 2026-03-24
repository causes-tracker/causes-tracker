use clap::Parser;

/// Runtime configuration for instance-api, read entirely from environment
/// variables (with `.env` file support via dotenvy).
#[derive(Parser, Debug, Clone)]
#[command(about = "Causes instance API server")]
pub struct Config {
    /// PostgreSQL connection string.
    #[arg(env = "DATABASE_URL")]
    pub database_url: String,

    /// Google OAuth 2.0 Client ID (TV and Limited Input devices type).
    /// Required during first-time bootstrap; can be unset after an admin
    /// has been created.
    #[arg(env = "GOOGLE_CLIENT_ID", default_value = "")]
    pub google_client_id: String,

    /// Google OAuth 2.0 Client Secret paired with `GOOGLE_CLIENT_ID`.
    #[arg(env = "GOOGLE_CLIENT_SECRET", default_value = "")]
    pub google_client_secret: String,

    /// Honeycomb API key for OpenTelemetry OTLP export.
    /// When absent, traces are not exported and only structured JSON logs
    /// are written to stdout.
    #[arg(env = "HONEYCOMB_API_KEY")]
    pub honeycomb_api_key: Option<String>,

    /// gRPC listen address.
    #[arg(env = "BIND_ADDR", default_value = "[::]:50051")]
    pub bind_addr: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bind_addr() {
        // Simulate minimal env: only DATABASE_URL set.
        let cfg = Config::parse_from([
            "instance-api",
            "--database-url=postgresql://test",
        ]);
        assert_eq!(cfg.bind_addr, "[::]:50051");
        assert!(cfg.honeycomb_api_key.is_none());
    }

    #[test]
    fn honeycomb_key_optional() {
        let cfg = Config::parse_from([
            "instance-api",
            "--database-url=postgresql://test",
            "--bind-addr=[::]:9090",
        ]);
        assert_eq!(cfg.bind_addr, "[::]:9090");
        assert!(cfg.honeycomb_api_key.is_none());
    }
}
