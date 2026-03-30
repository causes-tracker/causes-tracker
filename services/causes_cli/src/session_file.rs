use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// On-disk session token for a single server.
/// Stored in `<data_dir>/<sanitised_host>.json`.
#[derive(Serialize, Deserialize)]
pub struct SessionFile {
    pub session_token: String,
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

/// Derive a filesystem-safe filename from a server URL.
fn session_filename(server: &str) -> String {
    let sanitised: String = server
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
        .collect();
    format!("{sanitised}.json")
}

fn session_path(data_dir: &std::path::Path, server: &str) -> PathBuf {
    data_dir.join(session_filename(server))
}

pub fn load(data_dir: &std::path::Path, server: &str) -> anyhow::Result<Option<SessionFile>> {
    let path = session_path(data_dir, server);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).context("reading session file")?;
    let session: SessionFile = serde_json::from_str(&content).context("parsing session file")?;
    Ok(Some(session))
}

pub fn save(data_dir: &std::path::Path, server: &str, session: &SessionFile) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).context("creating data directory")?;
    let path = session_path(data_dir, server);
    let content = serde_json::to_string_pretty(session).context("serialising session")?;
    std::fs::write(&path, content).context("writing session file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("causes-{name}-{}", std::process::id()))
    }

    #[test]
    fn session_filename_sanitises_url() {
        assert_eq!(
            session_filename("https://causes.example.com"),
            "causes.example.com.json"
        );
        assert_eq!(session_filename("http://[::1]:50051"), "___1__50051.json");
        assert_eq!(
            session_filename("https://my-host.io:443"),
            "my-host.io_443.json"
        );
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = test_dir("roundtrip");
        let server = "http://localhost:50051";
        let session = SessionFile {
            session_token: "abc123".to_string(),
        };
        save(&dir, server, &session).expect("save failed");
        let loaded = load(&dir, server)
            .expect("load failed")
            .expect("no session file");
        assert_eq!(loaded.session_token, "abc123");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn different_servers_have_different_files() {
        let dir = test_dir("multi-server");
        save(
            &dir,
            "http://server-a:50051",
            &SessionFile {
                session_token: "token-a".to_string(),
            },
        )
        .expect("save a failed");
        save(
            &dir,
            "http://server-b:50051",
            &SessionFile {
                session_token: "token-b".to_string(),
            },
        )
        .expect("save b failed");
        let a = load(&dir, "http://server-a:50051")
            .expect("load a failed")
            .expect("no session for a");
        let b = load(&dir, "http://server-b:50051")
            .expect("load b failed")
            .expect("no session for b");
        assert_eq!(a.session_token, "token-a");
        assert_eq!(b.session_token, "token-b");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = test_dir("missing");
        let loaded = load(&dir, "http://nonexistent:1234").expect("load failed");
        assert!(loaded.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
