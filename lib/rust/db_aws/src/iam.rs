use aws_sdk_rds::auth_token;

/// Parameters needed to generate an IAM auth token for RDS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IamParams {
    pub hostname: String,
    pub port: u16,
    pub username: String,
}

impl IamParams {
    /// Create IAM parameters.
    pub fn new(hostname: String, port: u16, username: String) -> Self {
        Self {
            hostname,
            port,
            username,
        }
    }

    /// Generate a short-lived IAM auth token usable as a PostgreSQL password.
    ///
    /// The token is valid for up to 15 minutes, but an already-authenticated
    /// connection persists beyond token expiry.
    pub async fn generate_token(
        &self,
        sdk_config: &aws_types::SdkConfig,
    ) -> anyhow::Result<String> {
        let cfg = auth_token::Config::builder()
            .hostname(&self.hostname)
            .port(u64::from(self.port))
            .username(&self.username)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let generator = auth_token::AuthTokenGenerator::new(cfg);
        let token = generator
            .auth_token(sdk_config)
            .await
            .map_err(|e| anyhow::anyhow!("generating RDS IAM auth token: {e}"))?;
        Ok(token.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_credential_types::Credentials;
    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_smithy_async::test_util::ManualTimeSource;
    use std::time::{Duration, UNIX_EPOCH};

    #[tokio::test]
    async fn generate_token_returns_signed_url() {
        let time_source = ManualTimeSource::new(UNIX_EPOCH + Duration::from_secs(1_724_709_600));
        let sdk_config = aws_types::SdkConfig::builder()
            .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
                "AKID", "secret", None, None, "test",
            )))
            .time_source(time_source)
            .build();

        let params = IamParams::new(
            "mydb.us-east-1.rds.amazonaws.com".into(),
            5432,
            "causes".into(),
        );

        let token: String = params.generate_token(&sdk_config).await.expect("token");

        // Token must look like a presigned RDS connect URL (without the https:// prefix).
        assert!(
            token.starts_with("mydb.us-east-1.rds.amazonaws.com:5432/"),
            "token should start with host:port/: {token}"
        );
        assert!(
            token.contains("X-Amz-Credential="),
            "token should contain credential: {token}"
        );
        assert!(
            token.contains("Action=connect"),
            "token should contain connect action: {token}"
        );
    }
}
