use anyhow::Context;
use serde::Deserialize;

pub const DEVICE_AUTH_URL: &str = "https://oauth2.googleapis.com/device/code";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const TOKEN_INFO_URL: &str = "https://oauth2.googleapis.com/tokeninfo";

// ── Field newtypes ─────────────────────────────────────────────────────────

/// A non-empty device code returned by the device authorization endpoint.
#[derive(Debug, Clone)]
pub struct DeviceCode(String);

impl DeviceCode {
    fn new(s: String) -> anyhow::Result<Self> {
        anyhow::ensure!(!s.is_empty(), "device_code must not be empty");
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A non-empty user code displayed to the end user.
#[derive(Debug, Clone)]
pub struct UserCode(String);

impl UserCode {
    fn new(s: String) -> anyhow::Result<Self> {
        anyhow::ensure!(!s.is_empty(), "user_code must not be empty");
        Ok(Self(s))
    }
}

impl std::fmt::Display for UserCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A verified URL returned as `verification_url` by the device authorization endpoint.
#[derive(Debug, Clone)]
pub struct VerificationUrl(String);

impl VerificationUrl {
    fn new(s: String) -> anyhow::Result<Self> {
        // Validate that the string is a well-formed URL.
        let _parsed: reqwest::Url = s.parse().context("verification_url is not a valid URL")?;
        Ok(Self(s))
    }
}

impl std::fmt::Display for VerificationUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Response types ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct DeviceAuthResponse {
    pub device_code: DeviceCode,
    pub user_code: UserCode,
    pub verification_url: VerificationUrl,
    /// Server-specified polling interval, or `None` if the server did not provide one.
    pub interval: Option<u64>,
}

/// Raw deserialization target — converted to `DeviceAuthResponse` after validation.
#[derive(Deserialize)]
struct RawDeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_url: String,
    interval: Option<u64>,
}

/// A token response containing a JWT id_token.
#[derive(Debug)]
pub struct TokenResponse {
    pub id_token: String,
}

/// Raw deserialization target — validated into `TokenResponse`.
#[derive(Deserialize)]
struct RawTokenResponse {
    id_token: String,
}

impl TryFrom<RawTokenResponse> for TokenResponse {
    type Error = anyhow::Error;

    fn try_from(raw: RawTokenResponse) -> anyhow::Result<Self> {
        let parts: Vec<&str> = raw.id_token.split('.').collect();
        anyhow::ensure!(
            parts.len() == 3 && parts.iter().all(|p| !p.is_empty()),
            "id_token is not a valid JWT (expected 3 non-empty dot-separated segments)"
        );
        Ok(Self {
            id_token: raw.id_token,
        })
    }
}

#[derive(Deserialize, Debug)]
pub struct IdTokenClaims {
    pub sub: String,
    pub email: String,
    pub name: String,
    pub iss: String,
}

// ── HTTP helpers ───────────────────────────────────────────────────────────

pub async fn request_device_code(
    client: &reqwest::Client,
    client_id: &str,
    device_auth_url: &str,
) -> anyhow::Result<DeviceAuthResponse> {
    let raw = client
        .post(device_auth_url)
        .form(&[("client_id", client_id), ("scope", "openid email profile")])
        .send()
        .await
        .context("requesting device code")?
        .error_for_status()
        .context("device auth endpoint returned error")?
        .json::<RawDeviceAuthResponse>()
        .await
        .context("parsing device auth response")?;

    Ok(DeviceAuthResponse {
        device_code: DeviceCode::new(raw.device_code)?,
        user_code: UserCode::new(raw.user_code)?,
        verification_url: VerificationUrl::new(raw.verification_url)?,
        interval: raw.interval, // None if server didn't specify
    })
}

/// Poll the token endpoint until the user completes sign-in.
///
/// Implements RFC 8628 §3.5: on `slow_down` the polling interval is
/// permanently increased by 5 seconds for all subsequent requests.
pub async fn poll_for_token(
    client: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    device_code: &str,
    interval: Option<u64>,
    token_url: &str,
) -> anyhow::Result<TokenResponse> {
    // RFC 8628 §3.2: minimum polling interval is 5 seconds.
    let mut sleep_dur = std::time::Duration::from_secs(interval.unwrap_or(5).max(5));

    loop {
        tokio::time::sleep(sleep_dur).await;

        match try_token_once(client, client_id, client_secret, device_code, token_url).await? {
            TokenPollResult::Ready(token) => return Ok(token),
            TokenPollResult::Pending => continue,
            TokenPollResult::SlowDown => sleep_dur += std::time::Duration::from_secs(5),
        }
    }
}

/// Result of a single token-endpoint poll attempt.
pub enum TokenPollResult {
    /// The user has not yet completed sign-in.
    Pending,
    /// The server asked us to slow down — the caller should increase the interval.
    SlowDown,
    /// The token endpoint returned an id_token.
    Ready(TokenResponse),
}

/// Make a single attempt to exchange the device code for a token.
/// Unlike `poll_for_token`, this does NOT loop or sleep.
pub async fn try_token_once(
    client: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    device_code: &str,
    token_url: &str,
) -> anyhow::Result<TokenPollResult> {
    let resp = client
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .context("polling token endpoint")?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.context("parsing token poll response")?;

    if status.is_success() {
        let raw: RawTokenResponse =
            serde_json::from_value(body).context("deserialising token response")?;
        return Ok(TokenPollResult::Ready(raw.try_into()?));
    }

    match body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
    {
        "authorization_pending" => Ok(TokenPollResult::Pending),
        "slow_down" => Ok(TokenPollResult::SlowDown),
        other => anyhow::bail!("token endpoint error: {other}"),
    }
}

pub async fn validate_id_token(
    client: &reqwest::Client,
    id_token: &str,
    token_info_url: &str,
) -> anyhow::Result<IdTokenClaims> {
    client
        .get(token_info_url)
        .query(&[("id_token", id_token)])
        .send()
        .await
        .context("calling tokeninfo")?
        .error_for_status()
        .context("tokeninfo returned error")?
        .json::<IdTokenClaims>()
        .await
        .context("parsing tokeninfo response")
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    // ── Newtype validation ─────────────────────────────────────────────────

    #[test]
    fn device_code_rejects_empty() {
        assert!(DeviceCode::new(String::new()).is_err());
    }

    #[test]
    fn device_code_accepts_nonempty() {
        assert!(DeviceCode::new("abc123".to_string()).is_ok());
    }

    #[test]
    fn user_code_rejects_empty() {
        assert!(UserCode::new(String::new()).is_err());
    }

    #[test]
    fn user_code_accepts_nonempty() {
        let c = UserCode::new("ABCD-1234".to_string()).unwrap();
        assert_eq!(c.to_string(), "ABCD-1234");
    }

    #[test]
    fn verification_url_rejects_non_url() {
        assert!(VerificationUrl::new("not a url".to_string()).is_err());
    }

    #[test]
    fn verification_url_accepts_valid_url() {
        assert!(VerificationUrl::new("https://accounts.google.com/device".to_string()).is_ok());
    }

    #[test]
    fn interval_none_when_absent() {
        let raw: RawDeviceAuthResponse = serde_json::from_str(
            r#"{"device_code":"dc","user_code":"uc","verification_url":"https://example.com"}"#,
        )
        .unwrap();
        assert_eq!(raw.interval, None);
    }

    #[test]
    fn interval_uses_server_value_when_present() {
        let raw: RawDeviceAuthResponse = serde_json::from_str(
            r#"{"device_code":"dc","user_code":"uc","verification_url":"https://example.com","interval":10}"#,
        )
        .unwrap();
        assert_eq!(raw.interval, Some(10));
    }

    // ── JWT validation ─────────────────────────────────────────────────────

    #[test]
    fn token_response_rejects_non_jwt() {
        let raw = RawTokenResponse {
            id_token: "not-a-jwt".to_string(),
        };
        assert!(TokenResponse::try_from(raw).is_err());
    }

    #[test]
    fn token_response_rejects_two_segments() {
        let raw = RawTokenResponse {
            id_token: "header.payload".to_string(),
        };
        assert!(TokenResponse::try_from(raw).is_err());
    }

    #[test]
    fn token_response_rejects_empty_segment() {
        let raw = RawTokenResponse {
            id_token: "header..signature".to_string(),
        };
        assert!(TokenResponse::try_from(raw).is_err());
    }

    #[test]
    fn token_response_accepts_valid_jwt() {
        let raw = RawTokenResponse {
            id_token: "header.payload.signature".to_string(),
        };
        let resp = TokenResponse::try_from(raw).unwrap();
        assert_eq!(resp.id_token, "header.payload.signature");
    }

    // ── HTTP helpers ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn request_device_code_propagates_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device/code"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = request_device_code(
            &client,
            "client-id",
            &format!("{}/device/code", server.uri()),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn request_device_code_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/device/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code": "dev-code-xyz",
                "user_code": "ABCD-1234",
                "verification_url": "https://accounts.google.com/device",
                "interval": 5
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let resp = request_device_code(
            &client,
            "client-id",
            &format!("{}/device/code", server.uri()),
        )
        .await
        .unwrap();
        assert_eq!(resp.device_code.as_str(), "dev-code-xyz");
        assert_eq!(resp.user_code.to_string(), "ABCD-1234");
        assert_eq!(resp.interval, Some(5));
    }

    #[tokio::test]
    async fn validate_id_token_propagates_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = validate_id_token(
            &client,
            "fake-token",
            &format!("{}/tokeninfo", server.uri()),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn validate_id_token_parses_claims() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sub": "12345",
                "email": "user@example.com",
                "name": "Test User",
                "iss": "accounts.google.com"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let claims = validate_id_token(
            &client,
            "fake-token",
            &format!("{}/tokeninfo", server.uri()),
        )
        .await
        .unwrap();
        assert_eq!(claims.sub, "12345");
        assert_eq!(claims.email, "user@example.com");
        assert_eq!(claims.iss, "accounts.google.com");
    }
}
