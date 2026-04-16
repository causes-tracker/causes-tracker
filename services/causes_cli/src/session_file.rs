//! CLI session persistence — delegates to `causes_session`.

pub use causes_session::default_data_dir;

use causes_session::{FileSessionStore, SessionKind, SessionStorage};

/// Load the session token for a server, returning `None` if absent or invalid.
pub fn load(data_dir: &std::path::Path, server: &str) -> anyhow::Result<Option<String>> {
    FileSessionStore::new(SessionKind::Cli, data_dir, server).load()
}

/// Save a session token for a server.
pub fn save(data_dir: &std::path::Path, server: &str, token: &str) -> anyhow::Result<()> {
    FileSessionStore::new(SessionKind::Cli, data_dir, server).save(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let server = "http://localhost:50051";
        let token = "a".repeat(64);
        save(dir.path(), server, &token).expect("save failed");
        let loaded = load(dir.path(), server)
            .expect("load failed")
            .expect("no session");
        assert_eq!(loaded, token);
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(dir.path(), "http://nonexistent:1234").expect("load failed");
        assert!(loaded.is_none());
    }
}
