use anyhow::Context;
use tracing::info;

use crate::config::Config;
use crate::google;
use crate::store::Store;

/// Outcome of the bootstrap process.
#[derive(Debug, PartialEq, Eq)]
pub enum BootstrapResult {
    /// Users already exist — bootstrap was skipped.
    AlreadyBootstrapped,
    /// First admin was created successfully.
    AdminCreated,
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Run first-time bootstrap: request a Google device code, print the user-facing
/// code to stdout, poll until the user completes sign-in, then create the first
/// instance-admin record in the database.
pub async fn run(
    store: &impl Store,
    cfg: &Config,
    client: &reqwest::Client,
) -> anyhow::Result<BootstrapResult> {
    run_with_urls(
        store,
        cfg,
        client,
        google::DEVICE_AUTH_URL,
        google::TOKEN_URL,
        google::TOKEN_INFO_URL,
    )
    .await
}

pub async fn run_with_urls(
    store: &impl Store,
    cfg: &Config,
    client: &reqwest::Client,
    device_auth_url: &str,
    token_url: &str,
    token_info_url: &str,
) -> anyhow::Result<BootstrapResult> {
    let count = store.user_count().await.context("checking user count")?;

    if count > 0 {
        return Ok(BootstrapResult::AlreadyBootstrapped);
    }

    if cfg.google_client_id.is_empty() || cfg.google_client_secret.is_empty() {
        anyhow::bail!(
            "no administrators exist and GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET are not set; \
             set them to run first-time bootstrap"
        );
    }

    info!("no administrators found — starting bootstrap");

    let auth_resp =
        google::request_device_code(client, &cfg.google_client_id, device_auth_url).await?;

    println!();
    println!("No administrators configured.");
    println!(
        "Visit {} and enter code: {}",
        auth_resp.verification_url, auth_resp.user_code
    );
    println!();

    let token = google::poll_for_token(
        client,
        &cfg.google_client_id,
        &cfg.google_client_secret,
        auth_resp.device_code.as_str(),
        auth_resp.interval,
        token_url,
    )
    .await?;

    let claims = google::validate_id_token(client, &token.id_token, token_info_url).await?;

    let display_name = api_db::DisplayName::new(&claims.name)?;
    let email = api_db::Email::new(&claims.email)?;
    let auth_provider = api_db::AuthProvider::new(&claims.iss)?;
    let subject = api_db::Subject::new(&claims.sub)?;

    store
        .create_admin(&display_name, &email, &auth_provider, &subject)
        .await?;

    println!("Admin {} created. Instance is ready.", email);
    info!(email = %email, "bootstrap complete");

    Ok(BootstrapResult::AdminCreated)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    // ── FakeStore ──────────────────────────────────────────────────────────

    struct FakeStore {
        count: i64,
        created: Mutex<Vec<String>>,
    }

    impl FakeStore {
        fn empty() -> Self {
            Self {
                count: 0,
                created: Mutex::new(vec![]),
            }
        }

        fn with_users(n: i64) -> Self {
            Self {
                count: n,
                created: Mutex::new(vec![]),
            }
        }

        fn admins_created(&self) -> Vec<String> {
            self.created.lock().unwrap().clone()
        }
    }

    impl Store for FakeStore {
        async fn migrate(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn user_count(&self) -> anyhow::Result<i64> {
            Ok(self.count)
        }

        async fn create_admin(
            &self,
            _display_name: &api_db::DisplayName,
            email: &api_db::Email,
            _auth_provider: &api_db::AuthProvider,
            _subject: &api_db::Subject,
        ) -> anyhow::Result<api_db::UserId> {
            self.created.lock().unwrap().push(email.to_string());
            Ok(api_db::UserId::new())
        }
    }

    // ── Helpers ────────────────────────────────────────────────────────────

    fn test_cfg() -> Config {
        use clap::Parser;
        Config::parse_from([
            "causes_api",
            "--database-url=postgresql://unused",
            "--google-client-id=test-client-id",
            "--google-client-secret=test-client-secret",
        ])
    }

    fn no_creds_cfg() -> Config {
        use clap::Parser;
        Config::parse_from(["causes_api", "--database-url=postgresql://unused"])
    }

    // ── run_with_urls tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn returns_false_when_users_exist() {
        let store = FakeStore::with_users(1);
        let cfg = test_cfg();
        let client = reqwest::Client::new();

        // No mock server needed — should return before any HTTP call.
        let result = run_with_urls(
            &store,
            &cfg,
            &client,
            "http://unused/device",
            "http://unused/token",
            "http://unused/tokeninfo",
        )
        .await
        .expect("run_with_urls failed");

        assert_eq!(result, BootstrapResult::AlreadyBootstrapped);
        assert!(store.admins_created().is_empty());
    }

    #[tokio::test]
    async fn errors_when_both_credentials_missing() {
        let store = FakeStore::empty();
        let cfg = no_creds_cfg();
        let client = reqwest::Client::new();

        let err = run_with_urls(
            &store,
            &cfg,
            &client,
            "http://unused/device",
            "http://unused/token",
            "http://unused/tokeninfo",
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("GOOGLE_CLIENT_ID"), "{err}");
    }

    #[tokio::test]
    async fn errors_when_client_id_missing() {
        use clap::Parser;
        let store = FakeStore::empty();
        let cfg = Config::parse_from([
            "causes_api",
            "--database-url=postgresql://unused",
            "--google-client-secret=some-secret",
        ]);
        let client = reqwest::Client::new();

        let err = run_with_urls(
            &store,
            &cfg,
            &client,
            "http://unused/device",
            "http://unused/token",
            "http://unused/tokeninfo",
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("GOOGLE_CLIENT_ID"), "{err}");
    }

    #[tokio::test]
    async fn errors_when_client_secret_missing() {
        use clap::Parser;
        let store = FakeStore::empty();
        let cfg = Config::parse_from([
            "causes_api",
            "--database-url=postgresql://unused",
            "--google-client-id=some-id",
        ]);
        let client = reqwest::Client::new();

        let err = run_with_urls(
            &store,
            &cfg,
            &client,
            "http://unused/device",
            "http://unused/token",
            "http://unused/tokeninfo",
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("GOOGLE_CLIENT_SECRET"), "{err}");
    }

    #[tokio::test]
    async fn full_flow_creates_admin() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/device"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code": "dev-code",
                "user_code": "USER-CODE",
                "verification_url": "https://accounts.google.com/device",
                "interval": 5,
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "id_token": "test.id.token" })),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "uid-123",
                "email": "admin@example.com",
                "name": "Test Admin",
                "iss": "accounts.google.com",
            })))
            .mount(&mock_server)
            .await;

        let store = FakeStore::empty();
        let cfg = test_cfg();
        let client = reqwest::Client::new();
        let base = mock_server.uri();

        let result = run_with_urls(
            &store,
            &cfg,
            &client,
            &format!("{base}/device"),
            &format!("{base}/token"),
            &format!("{base}/tokeninfo"),
        )
        .await
        .expect("run_with_urls failed");

        assert_eq!(result, BootstrapResult::AdminCreated);
        assert_eq!(store.admins_created(), vec!["admin@example.com"]);
    }

    // ── poll_for_token tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn poll_cycles_pending_then_succeeds() {
        let mock_server = MockServer::start().await;
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(move |_req: &Request| {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({ "error": "authorization_pending" }))
                } else {
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "id_token": "test.id.token" }))
                }
            })
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let token_url = format!("{}/token", mock_server.uri());

        let token = google::poll_for_token(&client, "cid", "csecret", "dev_code", None, &token_url)
            .await
            .expect("poll_for_token failed");

        assert_eq!(token.id_token, "test.id.token");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn poll_slow_down_increases_interval() {
        // slow_down on the first call, success on the second.
        // We can't easily measure wall time in a unit test, but we can
        // verify the function succeeds after handling slow_down.
        let mock_server = MockServer::start().await;
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(move |_req: &Request| {
                let n = cc.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({ "error": "slow_down" }))
                } else {
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "id_token": "slowed.id.token" }))
                }
            })
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let token_url = format!("{}/token", mock_server.uri());

        let token = google::poll_for_token(&client, "cid", "csecret", "dev_code", None, &token_url)
            .await
            .expect("poll_for_token failed after slow_down");

        assert_eq!(token.id_token, "slowed.id.token");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn poll_errors_on_unknown_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({ "error": "access_denied" })),
            )
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let token_url = format!("{}/token", mock_server.uri());

        let err = google::poll_for_token(&client, "cid", "csecret", "dev_code", None, &token_url)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("access_denied"), "{err}");
    }

    // ── validate_id_token tests ────────────────────────────────────────────

    #[tokio::test]
    async fn validate_parses_claims() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "uid-123",
                "email": "admin@example.com",
                "name": "Test Admin",
                "iss": "accounts.google.com",
            })))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/tokeninfo", mock_server.uri());

        let claims = google::validate_id_token(&client, "fake-token", &url)
            .await
            .expect("validate_id_token failed");

        assert_eq!(claims.sub, "uid-123");
        assert_eq!(claims.email, "admin@example.com");
        assert_eq!(claims.iss, "accounts.google.com");
    }

    #[tokio::test]
    async fn validate_propagates_http_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad token"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/tokeninfo", mock_server.uri());

        let err = google::validate_id_token(&client, "bad-token", &url)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("tokeninfo"), "{err}");
    }

    // ── request_device_code tests ──────────────────────────────────────────

    #[tokio::test]
    async fn request_device_code_propagates_http_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/device"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/device", mock_server.uri());

        let err = google::request_device_code(&client, "bad-client-id", &url)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("device auth endpoint"), "{err}");
    }
}
