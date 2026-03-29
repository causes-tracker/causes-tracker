use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// On-disk session state stored in `~/.config/causes/session.json`.
#[derive(Serialize, Deserialize)]
pub struct SessionFile {
    pub session_token: String,
    pub server: String,
}

fn session_path() -> anyhow::Result<PathBuf> {
    let config_dir = dirs();
    std::fs::create_dir_all(&config_dir).context("creating config directory")?;
    Ok(config_dir.join("session.json"))
}

fn dirs() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join(".config")
        })
        .join("causes")
}

pub fn load() -> anyhow::Result<Option<SessionFile>> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).context("reading session file")?;
    let session: SessionFile = serde_json::from_str(&content).context("parsing session file")?;
    Ok(Some(session))
}

pub fn save(session: &SessionFile) -> anyhow::Result<()> {
    let path = session_path()?;
    let content = serde_json::to_string_pretty(session).context("serialising session")?;
    std::fs::write(&path, content).context("writing session file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_save_and_load() {
        let dir = std::env::temp_dir().join(format!("causes-test-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let session = SessionFile {
            session_token: "abc123".to_string(),
            server: "http://localhost:50051".to_string(),
        };

        save(&session).expect("save failed");
        let loaded = load().expect("load failed").expect("no session file");

        assert_eq!(loaded.session_token, "abc123");
        assert_eq!(loaded.server, "http://localhost:50051");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = std::env::temp_dir().join(format!("causes-test-missing-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        let loaded = load().expect("load failed");
        assert!(loaded.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
