//! AWS-aware connection setup for `db_pool::DbPool`. Kept out of `api_db`
//! so `sqlx prepare`'s `cargo check` doesn't pull in the AWS SDK.

mod iam;

use std::sync::Arc;
use std::time::Duration;

use db_pool::SetConnectOptions;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};

pub use iam::IamParams;

/// Build a `DbPool` using IAM authentication, loading default AWS SDK
/// configuration (env vars, instance profile, etc). The returned pool has
/// a refresher closure attached so [`db_pool::DbPool::start_background_refresh`]
/// rotates the IAM token on the production schedule.
#[tracing::instrument(skip(state), fields(db.system = "postgresql"))]
pub async fn connect_iam<E>(
    host: &str,
    port: u16,
    user: &str,
    state: E,
) -> anyhow::Result<db_pool::DbPool<E>> {
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
    connect_iam_with_sdk(params, sdk_config, state).await
}

/// How often the background refresher re-signs the IAM token.
/// Must stay below the 15-minute token lifetime RDS enforces, so the
/// pool's stored password is always a valid token when a reconnect
/// happens.
const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// How long a connection acquire may wait, covering Aurora Serverless
/// resume-from-auto-pause (observed up to ~34s).
/// Must stay below client budgets (browser fetch, gRPC callers) so the
/// caller sees the query outcome rather than its own timeout.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(60);

/// Same as [`connect_iam`] but takes a pre-built [`aws_types::SdkConfig`].
/// Use this when integrating with code that already has an SDK config
/// (shared credentials provider, custom region, test overrides).
#[tracing::instrument(skip(sdk_config, state), fields(db.system = "postgresql"))]
pub async fn connect_iam_with_sdk<E>(
    params: IamParams,
    sdk_config: aws_types::SdkConfig,
    state: E,
) -> anyhow::Result<db_pool::DbPool<E>> {
    let pool = build_iam_pool::<sqlx::PgPool>(&params, &sdk_config).await?;
    let refresher = iam_refresher(params, sdk_config);

    Ok(db_pool::DbPool::from_pool_with_refresher(
        pool,
        REFRESH_INTERVAL,
        refresher,
        state,
    ))
}

/// Build the refresher closure that rotates the IAM token of whatever
/// pool [`db_pool`] hands it, via [`set_pool_token`]. Live connections
/// are untouched; the next reconnect picks up the new password.
fn iam_refresher(params: IamParams, sdk_config: aws_types::SdkConfig) -> db_pool::PoolRefresher {
    // SdkConfig and IamParams are both Clone+Send+Sync, so the closure is too.
    Arc::new(move |pool| {
        let p = params.clone();
        let s = sdk_config.clone();
        Box::pin(async move { set_pool_token(pool, &p, &s).await })
    })
}

/// Construct the `PgConnectOptions` for an IAM-authenticated connection.
fn iam_connect_options(params: &IamParams, token: &str) -> PgConnectOptions {
    PgConnectOptions::new()
        .host(&params.hostname)
        .port(params.port)
        .username(&params.username)
        .password(token)
        .database("causes")
        .ssl_mode(PgSslMode::Require)
}

/// The full set of effectful pool operations the IAM flow performs,
/// behind a trait so tests can substitute a recorder and assert what
/// the flow actually passes. Verbs match the sqlx methods they
/// delegate to; production is `sqlx::PgPool` itself.
trait PoolOps: SetConnectOptions + Sized {
    fn connect_lazy_with(pool_opts: PgPoolOptions, connect_opts: PgConnectOptions) -> Self;
}

impl PoolOps for sqlx::PgPool {
    fn connect_lazy_with(pool_opts: PgPoolOptions, connect_opts: PgConnectOptions) -> Self {
        pool_opts.connect_lazy_with(connect_opts)
    }
}

/// The placeholder options are never used: the pool is lazy and the
/// real token is installed before anything can acquire.
async fn build_iam_pool<P: PoolOps>(
    params: &IamParams,
    sdk_config: &aws_types::SdkConfig,
) -> anyhow::Result<P> {
    let pool_opts = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(ACQUIRE_TIMEOUT);
    let pool = P::connect_lazy_with(pool_opts, PgConnectOptions::new());
    set_pool_token(&pool, params, sdk_config).await?;
    Ok(pool)
}

/// Token generation is local SigV4 signing — no network I/O.
async fn set_pool_token<P: SetConnectOptions + ?Sized>(
    pool: &P,
    params: &IamParams,
    sdk_config: &aws_types::SdkConfig,
) -> anyhow::Result<()> {
    let token = params.generate_token(sdk_config).await?;
    pool.set_connect_options(iam_connect_options(params, &token));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_credential_types::Credentials;
    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_smithy_async::test_util::ManualTimeSource;
    use std::time::UNIX_EPOCH;

    fn test_sdk_config() -> aws_types::SdkConfig {
        let time_source = ManualTimeSource::new(UNIX_EPOCH + Duration::from_secs(1_724_709_600));
        aws_types::SdkConfig::builder()
            .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
                "AKID", "secret", None, None, "test",
            )))
            .time_source(time_source)
            .build()
    }

    /// One recorded call on the [`PoolOps`] double, in order.
    enum Call {
        ConnectLazyWith(PgPoolOptions),
        SetConnectOptions(PgConnectOptions),
    }

    /// Test double recording what the IAM flow passes to the pool layer.
    #[derive(Default)]
    struct RecordingPool {
        calls: std::sync::Mutex<Vec<Call>>,
    }

    impl SetConnectOptions for RecordingPool {
        fn set_connect_options(&self, connect_opts: PgConnectOptions) {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetConnectOptions(connect_opts));
        }
    }

    impl PoolOps for RecordingPool {
        fn connect_lazy_with(pool_opts: PgPoolOptions, _connect_opts: PgConnectOptions) -> Self {
            let pool = Self::default();
            pool.calls
                .lock()
                .unwrap()
                .push(Call::ConnectLazyWith(pool_opts));
            pool
        }
    }

    fn test_params() -> IamParams {
        IamParams::new("db.invalid".into(), 5432, "causes".into())
    }

    #[tokio::test]
    async fn build_creates_pool_then_installs_token() {
        let pool = build_iam_pool::<RecordingPool>(&test_params(), &test_sdk_config())
            .await
            .unwrap();

        let calls = pool.calls.lock().unwrap();
        let (pool_opts, token_opts) = match calls.as_slice() {
            [
                Call::ConnectLazyWith(pool_opts),
                Call::SetConnectOptions(token_opts),
            ] => (pool_opts, token_opts),
            other => panic!("unexpected call sequence ({} calls)", other.len()),
        };
        // Above worst-case Aurora resume-from-pause (~34s observed),
        // below every client budget in the stack.
        assert_eq!(pool_opts.get_acquire_timeout(), ACQUIRE_TIMEOUT);
        assert!(ACQUIRE_TIMEOUT >= Duration::from_secs(45));
        assert!(ACQUIRE_TIMEOUT <= Duration::from_secs(90));
        assert_eq!(token_opts.get_host(), "db.invalid");
        assert_eq!(token_opts.get_username(), "causes");
    }

    #[tokio::test]
    async fn refresher_installs_token_on_the_pool_it_is_given() {
        let pool = RecordingPool::default();
        let refresher = iam_refresher(test_params(), test_sdk_config());
        refresher(&pool).await.unwrap();

        let calls = pool.calls.lock().unwrap();
        let token_opts = match calls.as_slice() {
            [Call::SetConnectOptions(token_opts)] => token_opts,
            _ => panic!("token refresh must install options exactly once"),
        };
        assert_eq!(token_opts.get_host(), "db.invalid");
    }

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
