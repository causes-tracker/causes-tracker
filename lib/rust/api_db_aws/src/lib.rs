//! AWS-aware connection setup for `api_db::DbPool`. Kept out of `api_db`
//! so `sqlx prepare`'s `cargo check` doesn't pull in the AWS SDK.

mod iam;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};

pub use iam::IamParams;

#[derive(Debug, PartialEq, Eq)]
enum AuthMode {
    Iam(IamParams),
    Static(String),
}

fn auth_mode(
    db_host: Option<&str>,
    db_user: Option<&str>,
    db_port: u16,
    database_url: Option<&str>,
) -> anyhow::Result<AuthMode> {
    match (db_host, db_user) {
        (Some(host), Some(user)) => Ok(AuthMode::Iam(IamParams::new(
            host.to_owned(),
            db_port,
            user.to_owned(),
        ))),
        _ => {
            let url = database_url
                .context("DATABASE_URL is required when DB_HOST/DB_USER are not set")?;
            Ok(AuthMode::Static(url.to_owned()))
        }
    }
}

/// Build a `DbPool` from environment-style configuration. Picks IAM auth
/// (production path) when `db_host` and `db_user` are both `Some`, otherwise
/// falls back to the static `database_url`.
#[tracing::instrument(skip(database_url), fields(db.system = "postgresql"))]
pub async fn from_config(
    db_host: Option<&str>,
    db_user: Option<&str>,
    db_port: u16,
    database_url: Option<&str>,
) -> anyhow::Result<api_db::DbPool> {
    match auth_mode(db_host, db_user, db_port, database_url)? {
        AuthMode::Iam(params) => {
            tracing::info!("using IAM database authentication");
            let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            connect_iam(params, sdk_config).await
        }
        AuthMode::Static(url) => api_db::DbPool::connect(&url).await,
    }
}

/// Build a `DbPool` using IAM authentication. The returned pool has a
/// refresher closure attached so [`api_db::DbPool::start_background_refresh`]
/// will rotate the IAM token on the production schedule.
#[tracing::instrument(skip(sdk_config), fields(db.system = "postgresql"))]
pub async fn connect_iam(
    params: IamParams,
    sdk_config: aws_types::SdkConfig,
) -> anyhow::Result<api_db::DbPool> {
    let pool = build_iam_pool(&params, &sdk_config).await?;

    // Capture state for the refresher closure. SdkConfig and IamParams are
    // both Clone+Send+Sync, so the closure is too.
    let p = params.clone();
    let s = sdk_config.clone();
    let refresher: api_db::PoolRefresher = Arc::new(move || {
        let p = p.clone();
        let s = s.clone();
        Box::pin(async move { build_iam_pool(&p, &s).await })
    });

    Ok(api_db::DbPool::from_pool_with_refresher(
        pool,
        Duration::from_secs(6 * 3600),
        refresher,
    ))
}

async fn build_iam_pool(
    params: &IamParams,
    sdk_config: &aws_types::SdkConfig,
) -> anyhow::Result<sqlx::PgPool> {
    let token = params.generate_token(sdk_config).await?;

    let options = PgConnectOptions::new()
        .host(&params.hostname)
        .port(params.port)
        .username(&params.username)
        .password(&token)
        .database("causes")
        .ssl_mode(PgSslMode::Require);

    PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("connecting to PostgreSQL with IAM token")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_mode_picks_iam_when_host_and_user_are_set() {
        let mode = auth_mode(Some("db.example"), Some("svc"), 5432, None).unwrap();
        assert_eq!(
            mode,
            AuthMode::Iam(IamParams::new("db.example".into(), 5432, "svc".into()))
        );
    }

    #[test]
    fn auth_mode_picks_iam_even_when_database_url_also_set() {
        // IAM takes precedence — production sets all three.
        let mode = auth_mode(Some("db.example"), Some("svc"), 5432, Some("postgres://x")).unwrap();
        assert!(matches!(mode, AuthMode::Iam(_)));
    }

    #[test]
    fn auth_mode_falls_back_to_static_url_when_host_missing() {
        let mode = auth_mode(None, Some("svc"), 5432, Some("postgres://localhost/db")).unwrap();
        assert_eq!(mode, AuthMode::Static("postgres://localhost/db".into()));
    }

    #[test]
    fn auth_mode_falls_back_to_static_url_when_user_missing() {
        let mode = auth_mode(
            Some("db.example"),
            None,
            5432,
            Some("postgres://localhost/db"),
        )
        .unwrap();
        assert_eq!(mode, AuthMode::Static("postgres://localhost/db".into()));
    }

    #[test]
    fn auth_mode_errors_when_no_credentials_and_no_url() {
        let err = auth_mode(None, None, 5432, None).unwrap_err();
        assert!(
            err.to_string().contains("DATABASE_URL is required"),
            "unexpected error: {err}"
        );
    }
}
