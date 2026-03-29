use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// On-disk session state stored in `<data_dir>/session.json`.
#[derive(Serialize, Deserialize)]
pub struct SessionFile {
    pub session_token: String,
    pub server: String,
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

fn session_path(config_dir: &std::path::Path) -> PathBuf {
    config_dir.join("session.json")
}

pub fn load(config_dir: &std::path::Path) -> anyhow::Result<Option<SessionFile>> {
    let path = session_path(config_dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).context("reading session file")?;
    let session: SessionFile = serde_json::from_str(&content).context("parsing session file")?;
    Ok(Some(session))
}

pub fn save(config_dir: &std::path::Path, session: &SessionFile) -> anyhow::Result<()> {
    std::fs::create_dir_all(config_dir).context("creating config directory")?;
    let path = session_path(config_dir);
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
    fn round_trip_save_and_load() {
        let dir = test_dir("roundtrip");

        let session = SessionFile {
            session_token: "abc123".to_string(),
            server: "http://localhost:50051".to_string(),
        };

        save(&dir, &session).expect("save failed");
        let loaded = load(&dir).expect("load failed").expect("no session file");

        assert_eq!(loaded.session_token, "abc123");
        assert_eq!(loaded.server, "http://localhost:50051");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = test_dir("missing");

        let loaded = load(&dir).expect("load failed");
        assert!(loaded.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
