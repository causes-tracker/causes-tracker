//! MCP-specific session file storage.
//!
//! Stores session tokens in `<data_dir>/<sanitised_server>_mcp.json` to avoid
//! colliding with the CLI's session files.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SessionFile {
    session_token: String,
}

fn session_path(data_dir: &Path, server: &str) -> PathBuf {
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
    data_dir.join(format!("{sanitised}_mcp.json"))
}

pub fn load(data_dir: &Path, server: &str) -> anyhow::Result<Option<String>> {
    let path = session_path(data_dir, server);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).context("reading MCP session file")?;
    let session: SessionFile =
        serde_json::from_str(&content).context("parsing MCP session file")?;
    Ok(Some(session.session_token))
}

pub fn save(data_dir: &Path, server: &str, token: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).context("creating data directory")?;
    let path = session_path(data_dir, server);
    let session = SessionFile {
        session_token: token.to_string(),
    };
    let content = serde_json::to_string_pretty(&session).context("serialising session")?;
    std::fs::write(&path, content).context("writing MCP session file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("causes-mcp-session-{name}-{}", std::process::id()))
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = test_dir("roundtrip");
        save(&dir, "http://localhost:50051", "tok123").unwrap();
        let loaded = load(&dir, "http://localhost:50051").unwrap().unwrap();
        assert_eq!(loaded, "tok123");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = test_dir("missing");
        let loaded = load(&dir, "http://nonexistent:1").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn filename_uses_mcp_suffix() {
        let path = session_path(Path::new("/tmp"), "https://causes.example.com");
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "causes.example.com_mcp.json"
        );
    }

    #[test]
    fn does_not_collide_with_cli_session() {
        // CLI uses <sanitised>.json, MCP uses <sanitised>_mcp.json.
        let path = session_path(Path::new("/tmp"), "http://localhost:50051");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.ends_with("_mcp.json"));
        assert!(!filename.ends_with(".json") || filename.contains("_mcp"));
    }
}
