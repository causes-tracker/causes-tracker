use std::sync::Arc;
use std::time::Duration;

use tonic::{Request, Response, Status};
use tracing::info;

use causes_proto::auth_service_server::AuthService;
use causes_proto::{
    CompleteLoginRequest, CompleteLoginResponse, Pending, SessionCreated, StartLoginRequest,
    StartLoginResponse, WhoAmIRequest, WhoAmIResponse, complete_login_response,
};

use crate::config::Config;
use crate::google;

/// Shared state for the AuthService implementation.
///
/// Generic over the Google endpoint URLs so tests can point at wiremock.
pub struct AuthHandler<S> {
    store: Arc<S>,
    config: Arc<Config>,
    http: reqwest::Client,
    device_auth_url: String,
    token_url: String,
    token_info_url: String,
}

impl<S: crate::store::Store> AuthHandler<S> {
    pub fn new(store: Arc<S>, config: Arc<Config>, http: reqwest::Client) -> Self {
        Self {
            store,
            config,
            http,
            device_auth_url: google::DEVICE_AUTH_URL.to_owned(),
            token_url: google::TOKEN_URL.to_owned(),
            token_info_url: google::TOKEN_INFO_URL.to_owned(),
        }
    }
}

/// Default session duration: 30 days.
const SESSION_DURATION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[tonic::async_trait]
impl<S: crate::store::Store> AuthService for AuthHandler<S> {
    #[tracing::instrument(skip(self, _request))]
    async fn start_login(
        &self,
        _request: Request<StartLoginRequest>,
    ) -> Result<Response<StartLoginResponse>, Status> {
        let auth_resp = google::request_device_code(
            &self.http,
            &self.config.google_client_id,
            &self.device_auth_url,
        )
        .await
        .map_err(|e| Status::internal(format!("device code request failed: {e}")))?;

        let interval_secs = auth_resp.interval.unwrap_or(5).max(5) as i32;

        let nonce = self
            .store
            .create_pending_login(auth_resp.device_code.as_str(), interval_secs)
            .await
            .map_err(|e| Status::internal(format!("creating pending login: {e}")))?;

        info!("started device-flow login");

        Ok(Response::new(StartLoginResponse {
            nonce: nonce.as_str().to_owned(),
            user_code: auth_resp.user_code.to_string(),
            verification_url: auth_resp.verification_url.to_string(),
            interval_secs,
        }))
    }

    #[tracing::instrument(skip(self, request))]
    async fn complete_login(
        &self,
        request: Request<CompleteLoginRequest>,
    ) -> Result<Response<CompleteLoginResponse>, Status> {
        let nonce = api_db::LoginNonce::from_raw(request.into_inner().nonce)
            .map_err(|e| Status::invalid_argument(format!("invalid nonce: {e}")))?;

        let pending = self
            .store
            .lookup_pending_login(&nonce)
            .await
            .map_err(|e| Status::internal(format!("looking up pending login: {e}")))?
            .ok_or_else(|| Status::not_found("unknown or expired login nonce"))?;

        let poll_result = google::try_token_once(
            &self.http,
            &self.config.google_client_id,
            &self.config.google_client_secret,
            &pending.device_code,
            &self.token_url,
        )
        .await
        .map_err(|e| Status::internal(format!("token poll failed: {e}")))?;

        match poll_result {
            google::TokenPollResult::Pending | google::TokenPollResult::SlowDown => {
                Ok(Response::new(CompleteLoginResponse {
                    result: Some(complete_login_response::Result::Pending(Pending {})),
                }))
            }
            google::TokenPollResult::Ready(token_resp) => {
                let claims = google::validate_id_token(
                    &self.http,
                    &token_resp.id_token,
                    &self.token_info_url,
                )
                .await
                .map_err(|e| Status::internal(format!("validating id_token: {e}")))?;

                // Canonicalise the issuer the same way bootstrap does when
                // storing it, so the lookup matches regardless of whether
                // Google returns a bare hostname or a full URL.
                let issuer = api_db::AuthProvider::new(&claims.iss)
                    .map_err(|e| Status::internal(format!("invalid issuer: {e}")))?;

                let user_id = self
                    .store
                    .find_user_by_identity(issuer.as_str(), &claims.sub)
                    .await
                    .map_err(|e| Status::internal(format!("looking up user: {e}")))?
                    .ok_or_else(|| {
                        Status::permission_denied("no local account for this identity")
                    })?;

                // Delete nonce before creating session: if interrupted between
                // the two, the user has no session but the nonce is consumed —
                // they just log in again.  The reverse would leave a consumed
                // nonce in the DB alongside a valid session.
                self.store.delete_pending_login(&nonce).await.ok();

                let session_token = self
                    .store
                    .create_session(&user_id, SESSION_DURATION)
                    .await
                    .map_err(|e| Status::internal(format!("creating session: {e}")))?;

                info!("login complete");

                Ok(Response::new(CompleteLoginResponse {
                    result: Some(complete_login_response::Result::SessionCreated(
                        SessionCreated {
                            session_token: session_token.as_str().to_owned(),
                        },
                    )),
                }))
            }
        }
    }

    #[tracing::instrument(skip(self, _request))]
    async fn who_am_i(
        &self,
        _request: Request<WhoAmIRequest>,
    ) -> Result<Response<WhoAmIResponse>, Status> {
        Err(Status::unimplemented("WhoAmI not yet implemented"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MockStore;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> Config {
        use clap::Parser;
        Config::parse_from([
            "causes_api",
            "--database-url=postgresql://unused",
            "--google-client-id=test-client-id",
            "--google-client-secret=test-client-secret",
        ])
    }

    fn handler_with_urls(store: MockStore, server_uri: &str) -> AuthHandler<MockStore> {
        AuthHandler {
            store: Arc::new(store),
            config: Arc::new(test_config()),
            http: reqwest::Client::new(),
            device_auth_url: format!("{server_uri}/device"),
            token_url: format!("{server_uri}/token"),
            token_info_url: format!("{server_uri}/tokeninfo"),
        }
    }

    #[tokio::test]
    async fn start_login_returns_user_code_and_nonce() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code": "dev-code-123",
                "user_code": "ABCD-1234",
                "verification_url": "https://accounts.google.com/device",
                "interval": 5
            })))
            .mount(&server)
            .await;

        let nonce = api_db::LoginNonce::from_raw("a".repeat(64)).unwrap();
        let mut store = MockStore::new();
        store
            .expect_create_pending_login()
            .returning(move |_, _| Ok(nonce.clone()));

        let handler = handler_with_urls(store, &server.uri());
        let resp = handler
            .start_login(Request::new(StartLoginRequest {}))
            .await
            .expect("start_login failed")
            .into_inner();

        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.verification_url, "https://accounts.google.com/device");
        assert_eq!(resp.nonce.len(), 64);
        assert_eq!(resp.interval_secs, 5);
    }

    #[tokio::test]
    async fn complete_login_returns_pending_when_authorization_pending() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "authorization_pending"})),
            )
            .mount(&server)
            .await;

        let nonce_str = "b".repeat(64);
        let mut store = MockStore::new();
        store.expect_lookup_pending_login().returning(|_| {
            Ok(Some(api_db::PendingLoginRow {
                device_code: "dev-code".to_string(),
                interval_secs: 5,
            }))
        });

        let handler = handler_with_urls(store, &server.uri());
        let resp = handler
            .complete_login(Request::new(CompleteLoginRequest { nonce: nonce_str }))
            .await
            .expect("complete_login failed")
            .into_inner();

        assert!(matches!(
            resp.result,
            Some(complete_login_response::Result::Pending(_))
        ));
    }

    #[tokio::test]
    async fn complete_login_returns_session_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id_token": "hdr.payload.sig"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "uid-42",
                "email": "user@example.com",
                "name": "Test User",
                "iss": "accounts.google.com"
            })))
            .mount(&server)
            .await;

        let nonce_str = "c".repeat(64);
        let user_id = api_db::UserId::new();
        let token = api_db::SessionToken::from_raw("d".repeat(64)).unwrap();
        let uid = user_id.clone();
        let tok = token.clone();

        let mut store = MockStore::new();
        store.expect_lookup_pending_login().returning(|_| {
            Ok(Some(api_db::PendingLoginRow {
                device_code: "dev-code".to_string(),
                interval_secs: 5,
            }))
        });
        store
            .expect_find_user_by_identity()
            .returning(move |_, _| Ok(Some(uid.clone())));
        store
            .expect_create_session()
            .returning(move |_, _| Ok(tok.clone()));
        store.expect_delete_pending_login().returning(|_| Ok(()));

        let handler = handler_with_urls(store, &server.uri());
        let resp = handler
            .complete_login(Request::new(CompleteLoginRequest { nonce: nonce_str }))
            .await
            .expect("complete_login failed")
            .into_inner();

        match resp.result {
            Some(complete_login_response::Result::SessionCreated(sc)) => {
                assert_eq!(sc.session_token, "d".repeat(64));
            }
            other => panic!("expected SessionCreated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_login_rejects_unknown_nonce() {
        let store = MockStore::new();
        // MockStore default: lookup_pending_login not set → panic.
        // We need it to return None.
        let mut store = store;
        store.expect_lookup_pending_login().returning(|_| Ok(None));

        let handler = handler_with_urls(store, "http://unused");
        let err = handler
            .complete_login(Request::new(CompleteLoginRequest {
                nonce: "e".repeat(64),
            }))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn complete_login_rejects_invalid_nonce() {
        let store = MockStore::new();
        let handler = handler_with_urls(store, "http://unused");
        let err = handler
            .complete_login(Request::new(CompleteLoginRequest {
                nonce: "short".to_string(),
            }))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn complete_login_rejects_unknown_identity() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id_token": "hdr.payload.sig"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "unknown-sub",
                "email": "nobody@example.com",
                "name": "Nobody",
                "iss": "accounts.google.com"
            })))
            .mount(&server)
            .await;

        let mut store = MockStore::new();
        store.expect_lookup_pending_login().returning(|_| {
            Ok(Some(api_db::PendingLoginRow {
                device_code: "dev-code".to_string(),
                interval_secs: 5,
            }))
        });
        store
            .expect_find_user_by_identity()
            .returning(|_, _| Ok(None));

        let handler = handler_with_urls(store, &server.uri());
        let err = handler
            .complete_login(Request::new(CompleteLoginRequest {
                nonce: "f".repeat(64),
            }))
            .await
            .unwrap_err();

        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }
}
