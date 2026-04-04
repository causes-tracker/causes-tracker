use anyhow::Context;
use clap::Subcommand;

use causes_proto::auth_service_client::AuthServiceClient;
use causes_proto::{CompleteLoginRequest, StartLoginRequest, complete_login_response};

use crate::session_file::{self, SessionFile};

/// Authentication commands.
#[derive(clap::Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Log in to a Causes instance via device authorization flow.
    Login {
        /// Request an unrestricted session with full admin powers (1 hour).
        /// Default: restricted session with admin roles suppressed (30 days).
        #[arg(long)]
        admin: bool,
    },
    /// Show the currently authenticated user.
    #[command(name = "whoami")]
    WhoAmI,
}

pub async fn run(server: &str, data_dir: &std::path::Path, args: AuthArgs) -> anyhow::Result<()> {
    match args.command {
        AuthCommand::Login { admin } => login(server, data_dir, admin).await,
        AuthCommand::WhoAmI => whoami(server, data_dir).await,
    }
}

async fn login(server: &str, data_dir: &std::path::Path, admin: bool) -> anyhow::Result<()> {
    let mut client = AuthServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    let resp = client
        .start_login(StartLoginRequest {})
        .await
        .context("StartLogin RPC failed")?
        .into_inner();

    println!();
    println!("To sign in, open this URL in your browser:");
    println!();
    println!("  {}", resp.verification_url);
    println!();
    println!("Then enter code: {}", resp.user_code);
    println!();

    let interval = std::time::Duration::from_secs(resp.interval_secs.max(1) as u64);

    loop {
        tokio::time::sleep(interval).await;

        let poll_resp = client
            .complete_login(CompleteLoginRequest {
                nonce: resp.nonce.clone(),
                admin,
            })
            .await
            .context("CompleteLogin RPC failed")?
            .into_inner();

        match poll_resp.result {
            Some(complete_login_response::Result::Pending(_)) => {
                continue;
            }
            Some(complete_login_response::Result::SessionCreated(sc)) => {
                session_file::save(
                    data_dir,
                    server,
                    &SessionFile {
                        session_token: sc.session_token,
                    },
                )?;
                println!("Login successful. Session saved.");
                return Ok(());
            }
            None => {
                anyhow::bail!("unexpected empty response from CompleteLogin");
            }
        }
    }
}

async fn whoami(server: &str, data_dir: &std::path::Path) -> anyhow::Result<()> {
    let req = crate::rpc::authed_request(data_dir, server, causes_proto::WhoAmIRequest {})?;

    let mut client = AuthServiceClient::connect(server.to_owned())
        .await
        .context("connecting to server")?;

    let resp = client
        .who_am_i(req)
        .await
        .context("WhoAmI RPC failed")?
        .into_inner();

    println!("User ID:      {}", resp.user_id);
    println!("Display name: {}", resp.display_name);
    println!("Email:        {}", resp.email);
    if resp.admin {
        println!("Session:      admin (unrestricted)");
    } else {
        println!("Session:      restricted");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::Parser;

    use causes_proto::auth_service_server::{AuthService, AuthServiceServer};
    use causes_proto::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn auth_login_parses() {
        let cli = Cli::parse_from(["causes", "auth", "login"]);
        assert!(matches!(cli.command, crate::Command::Auth(_)));
    }

    #[test]
    fn auth_login_admin_parses() {
        let cli = Cli::parse_from(["causes", "auth", "login", "--admin"]);
        assert!(matches!(cli.command, crate::Command::Auth(_)));
    }

    #[test]
    fn auth_whoami_parses() {
        let cli = Cli::parse_from(["causes", "auth", "whoami"]);
        assert!(matches!(cli.command, crate::Command::Auth(_)));
    }

    #[test]
    fn auth_requires_subcommand() {
        let result = Cli::try_parse_from(["causes", "auth"]);
        assert!(result.is_err());
    }

    struct MockAuthService {
        poll_count: AtomicU32,
    }

    impl MockAuthService {
        fn new() -> Self {
            Self {
                poll_count: AtomicU32::new(0),
            }
        }
    }

    #[tonic::async_trait]
    impl AuthService for MockAuthService {
        async fn start_login(
            &self,
            _req: tonic::Request<StartLoginRequest>,
        ) -> Result<tonic::Response<StartLoginResponse>, tonic::Status> {
            Ok(tonic::Response::new(StartLoginResponse {
                nonce: "a".repeat(64),
                user_code: "TEST-CODE".to_string(),
                verification_url: "https://example.com/device".to_string(),
                interval_secs: 1,
            }))
        }

        async fn complete_login(
            &self,
            _req: tonic::Request<CompleteLoginRequest>,
        ) -> Result<tonic::Response<CompleteLoginResponse>, tonic::Status> {
            let n = self.poll_count.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Ok(tonic::Response::new(CompleteLoginResponse {
                    result: Some(complete_login_response::Result::Pending(Pending {})),
                }))
            } else {
                Ok(tonic::Response::new(CompleteLoginResponse {
                    result: Some(complete_login_response::Result::SessionCreated(
                        SessionCreated {
                            session_token: "d".repeat(64),
                        },
                    )),
                }))
            }
        }

        async fn who_am_i(
            &self,
            _req: tonic::Request<WhoAmIRequest>,
        ) -> Result<tonic::Response<WhoAmIResponse>, tonic::Status> {
            Ok(tonic::Response::new(WhoAmIResponse {
                user_id: "uid-42".to_string(),
                display_name: "Test User".to_string(),
                email: "test@example.com".to_string(),
                admin: false,
            }))
        }
    }

    async fn start_mock_server() -> String {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };

        let mock = Arc::new(MockAuthService::new());
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(AuthServiceServer::from_arc(mock))
                .serve(format!("127.0.0.1:{port}").parse().unwrap())
                .await
                .unwrap();
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn login_flow_polls_and_saves_token() {
        let server_url = start_mock_server().await;
        let dir = tempfile::tempdir().unwrap();

        super::login(&server_url, dir.path(), false)
            .await
            .expect("login failed");

        let session = crate::session_file::load(dir.path(), &server_url)
            .expect("load failed")
            .expect("no session saved");
        assert_eq!(session.session_token, "d".repeat(64));
    }

    #[tokio::test]
    async fn whoami_returns_user_info() {
        let server_url = start_mock_server().await;
        let dir = tempfile::tempdir().unwrap();

        crate::session_file::save(
            dir.path(),
            &server_url,
            &crate::session_file::SessionFile {
                session_token: "e".repeat(64),
            },
        )
        .expect("save failed");

        super::whoami(&server_url, dir.path())
            .await
            .expect("whoami failed");
    }

    #[tokio::test]
    async fn whoami_rejects_when_not_logged_in() {
        let dir = tempfile::tempdir().unwrap();

        let err = super::whoami("http://127.0.0.1:1", dir.path())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not logged in"));
    }
}
