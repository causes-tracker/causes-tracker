//! AWS-aware connection setup for `db_pool::DbPool`. Kept out of `api_db`
//! so `sqlx prepare`'s `cargo check` doesn't pull in the AWS SDK.

mod iam;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};

pub use iam::IamParams;

/// Build a `DbPool` using IAM authentication, loading default AWS SDK
/// configuration (env vars, instance profile, etc). The returned pool has
/// a refresher closure attached so [`db_pool::DbPool::start_background_refresh`]
/// rotates the IAM token on the production schedule.
#[tracing::instrument(fields(db.system = "postgresql"))]
pub async fn connect_iam(host: &str, port: u16, user: &str) -> anyhow::Result<db_pool::DbPool> {
    // `aws-config`'s default-https-client feature is disabled at the workspace level (see Cargo.toml) so that the SDK does not silently pull in `rustls-aws-lc` and double up rustls providers — every other workspace member uses ring. We provide the HttpClient explicitly here with the ring CryptoMode.
    use aws_smithy_http_client::{Builder, tls};
    let http_client = Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_https();
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .http_client(http_client)
        .load()
        .await;
    let params = IamParams::new(host.to_owned(), port, user.to_owned());
    connect_iam_with_sdk(params, sdk_config).await
}

/// Same as [`connect_iam`] but takes a pre-built [`aws_types::SdkConfig`].
/// Use this when integrating with code that already has an SDK config
/// (shared credentials provider, custom region, test overrides).
#[tracing::instrument(skip(sdk_config), fields(db.system = "postgresql"))]
pub async fn connect_iam_with_sdk(
    params: IamParams,
    sdk_config: aws_types::SdkConfig,
) -> anyhow::Result<db_pool::DbPool> {
    let pool = build_iam_pool(&params, &sdk_config).await?;

    // SdkConfig and IamParams are both Clone+Send+Sync, so the closure is too.
    let p = params.clone();
    let s = sdk_config.clone();
    let refresher: db_pool::PoolRefresher = Arc::new(move || {
        let p = p.clone();
        let s = s.clone();
        Box::pin(async move { build_iam_pool(&p, &s).await })
    });

    Ok(db_pool::DbPool::from_pool_with_refresher(
        pool,
        Duration::from_secs(6 * 3600),
        refresher,
    ))
}

/// Construct the `PgConnectOptions` for an IAM-authenticated connection.
/// Pulled out of [`build_iam_pool`] so the field-construction logic is
/// directly testable; the actual `PgPoolOptions::connect_with` call is a
/// thin sqlx pass-through that needs a live database to exercise.
fn iam_connect_options(params: &IamParams, token: &str) -> PgConnectOptions {
    PgConnectOptions::new()
        .host(&params.hostname)
        .port(params.port)
        .username(&params.username)
        .password(token)
        .database("causes")
        .ssl_mode(PgSslMode::Require)
}

async fn build_iam_pool(
    params: &IamParams,
    sdk_config: &aws_types::SdkConfig,
) -> anyhow::Result<sqlx::PgPool> {
    let token = params.generate_token(sdk_config).await?;
    PgPoolOptions::new()
        .max_connections(5)
        .connect_with(iam_connect_options(params, &token))
        .await
        .context("connecting to PostgreSQL with IAM token")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iam_connect_options_carries_params_through() {
        let params = IamParams::new(
            "mydb.us-east-1.rds.amazonaws.com".into(),
            5432,
            "svc".into(),
        );
        let opts = iam_connect_options(&params, "iam-token-123");
        assert_eq!(opts.get_host(), "mydb.us-east-1.rds.amazonaws.com");
        assert_eq!(opts.get_port(), 5432);
        assert_eq!(opts.get_username(), "svc");
        assert_eq!(opts.get_database(), Some("causes"));
        assert!(matches!(opts.get_ssl_mode(), PgSslMode::Require));
    }
}
