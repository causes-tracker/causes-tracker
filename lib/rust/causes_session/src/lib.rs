//! Session token persistence for Causes clients.
//!
//! Tokens are stored as JSON files in a data directory, keyed by server URL.
//! Different callers use different suffixes to avoid collisions.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Session tokens are two concatenated UUIDv4 hex strings (64 hex chars).
fn is_valid_token(token: &str) -> bool {
    token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit())
}

/// Derive a filesystem-safe stem from a server URL.
fn sanitise_server(server: &str) -> String {
    server
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Default data directory for session tokens and credentials.
/// Per XDG Base Directory spec, credentials belong in `$XDG_DATA_HOME`
/// (default `~/.local/share`), not `$XDG_CONFIG_HOME`.
pub fn default_data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join(".local/share")
        })
        .join("causes")
}

#[derive(Serialize, Deserialize)]
struct SessionFile {
    session_token: String,
}

/// Which client is storing the session.
pub enum SessionKind {
    Cli,
    Mcp,
}

impl SessionKind {
    fn suffix(&self) -> &'static str {
        match self {
            SessionKind::Cli => "",
            SessionKind::Mcp => "_mcp",
        }
    }
}

/// A session store that persists tokens to `<data_dir>/<sanitised_url><suffix>.json`.
#[derive(Clone)]
pub struct SessionStore {
    data_dir: PathBuf,
    server: String,
    suffix: &'static str,
}

impl SessionStore {
    pub fn new(kind: SessionKind, data_dir: &Path, server: &str) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            server: server.to_string(),
            suffix: kind.suffix(),
        }
    }

    fn path(&self) -> PathBuf {
        let stem = sanitise_server(&self.server);
        self.data_dir.join(format!("{stem}{}.json", self.suffix))
    }

    /// Load a session token, returning `None` if no file exists or the
    /// token is invalid.
    pub fn load(&self) -> anyhow::Result<Option<String>> {
        let path = self.path();
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path).context("reading session file")?;
        let session: SessionFile =
            serde_json::from_str(&content).context("parsing session file")?;
        if !is_valid_token(&session.session_token) {
            return Ok(None);
        }
        Ok(Some(session.session_token))
    }

    /// Save a session token.
    pub fn save(&self, token: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.data_dir).context("creating data directory")?;
        let session = SessionFile {
            session_token: token.to_string(),
        };
        let content = serde_json::to_string_pretty(&session).context("serialising session")?;
        std::fs::write(self.path(), content).context("writing session file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitise_strips_scheme() {
        assert_eq!(sanitise_server("https://example.com"), "example.com");
        assert_eq!(sanitise_server("http://localhost:50051"), "localhost_50051");
    }

    #[test]
    fn cli_and_mcp_use_different_files() {
        let cli = SessionStore::new(SessionKind::Cli, Path::new("/tmp"), "https://example.com");
        let mcp = SessionStore::new(SessionKind::Mcp, Path::new("/tmp"), "https://example.com");
        assert_ne!(cli.path(), mcp.path());
        assert!(cli.path().to_str().unwrap().ends_with("example.com.json"));
        assert!(
            mcp.path()
                .to_str()
                .unwrap()
                .ends_with("example.com_mcp.json")
        );
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let token = "d".repeat(64);
        let store = SessionStore::new(SessionKind::Mcp, dir.path(), "http://localhost:50051");
        store.save(&token).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), token);
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(SessionKind::Cli, dir.path(), "http://nonexistent:1");
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn load_returns_none_for_invalid_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(SessionKind::Mcp, dir.path(), "http://localhost:50051");

        std::fs::write(store.path(), r#"{"session_token":""}"#).unwrap();
        assert!(store.load().unwrap().is_none());

        std::fs::write(store.path(), r#"{"session_token":"abcd1234"}"#).unwrap();
        assert!(store.load().unwrap().is_none());

        let bad = "g".repeat(64);
        std::fs::write(store.path(), format!(r#"{{"session_token":"{bad}"}}"#)).unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn different_servers_have_different_files() {
        let dir = tempfile::tempdir().unwrap();
        let token_a = "a".repeat(64);
        let token_b = "b".repeat(64);
        let store_a = SessionStore::new(SessionKind::Cli, dir.path(), "http://server-a:50051");
        let store_b = SessionStore::new(SessionKind::Cli, dir.path(), "http://server-b:50051");
        store_a.save(&token_a).unwrap();
        store_b.save(&token_b).unwrap();
        assert_eq!(store_a.load().unwrap().unwrap(), token_a);
        assert_eq!(store_b.load().unwrap().unwrap(), token_b);
    }
}
