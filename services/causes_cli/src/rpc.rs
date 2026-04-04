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

    #[test]
    fn authed_request_fails_when_not_logged_in() {
        let dir = tempfile::tempdir().unwrap();
        let err = authed_request(dir.path(), "http://localhost:50051", ()).unwrap_err();
        assert!(err.to_string().contains("not logged in"));
    }

    #[test]
    fn authed_request_sets_bearer_header() {
        let dir = tempfile::tempdir().unwrap();
        session_file::save(
            dir.path(),
            "http://localhost:50051",
            &session_file::SessionFile {
                session_token: "a".repeat(64),
            },
        )
        .unwrap();

        let req = authed_request(dir.path(), "http://localhost:50051", ()).unwrap();
        let auth = req
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, format!("Bearer {}", "a".repeat(64)));
    }
}
