//! Shared helpers for making authenticated gRPC calls.

use anyhow::Context;

use crate::session_file;

/// Load the session token and build an authenticated `tonic::Request`.
///
/// Loads the stored session for `server`, inserts the Bearer token into
/// request metadata, and returns the ready-to-send request.
pub fn authed_request<T>(
    data_dir: &std::path::Path,
    server: &str,
    inner: T,
) -> anyhow::Result<tonic::Request<T>> {
    let session = session_file::load(data_dir, server)?
        .ok_or_else(|| anyhow::anyhow!("not logged in — run `causes auth login` first"))?;

    let mut req = tonic::Request::new(inner);
    req.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", session.session_token)
            .parse()
            .context("invalid session token")?,
    );
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("causes-rpc-{name}-{}", std::process::id()))
    }

    #[test]
    fn authed_request_sets_bearer_token() {
        let dir = test_dir("bearer");
        let server = "http://localhost:50051";
        session_file::save(
            &dir,
            server,
            &session_file::SessionFile {
                session_token: "a".repeat(64),
            },
        )
        .unwrap();

        let req = authed_request(&dir, server, ()).unwrap();
        let auth = req
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, format!("Bearer {}", "a".repeat(64)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn authed_request_fails_when_not_logged_in() {
        let dir = test_dir("no-session");
        let err = authed_request(&dir, "http://nowhere:1", ()).unwrap_err();
        assert!(err.to_string().contains("not logged in"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
