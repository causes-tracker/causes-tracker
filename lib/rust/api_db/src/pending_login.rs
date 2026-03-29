use anyhow::Context;
use sqlx::types::chrono;
use uuid::Uuid;

use crate::DbPool;

// ── Newtypes ──────────────────────────────────────────────────────────────

/// Single-use nonce that ties a StartLogin call to its CompleteLogin poll.
/// Same format as `SessionToken`: 64 hex characters (32 random bytes).
///
/// Deliberately does NOT implement `Display` — the nonce is a secret and
/// must not appear in logs or error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginNonce(String);

impl LoginNonce {
    fn generate() -> Self {
        let a = Uuid::new_v4().simple().to_string();
        let b = Uuid::new_v4().simple().to_string();
        Self(format!("{a}{b}"))
    }

    /// Wrap an existing hex string (e.g. from a gRPC request).
    pub fn from_raw(s: String) -> anyhow::Result<Self> {
        anyhow::ensure!(s.len() == 64, "login nonce must be 64 hex characters");
        anyhow::ensure!(
            s.chars().all(|c| c.is_ascii_hexdigit()),
            "login nonce must contain only hex characters"
        );
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── Public API ────────────────────────────────────────────────────────────

/// Persist a pending device-flow login and return the nonce.
pub async fn create_pending_login(
    pool: &DbPool,
    device_code: &str,
    interval_secs: i32,
    duration: std::time::Duration,
) -> anyhow::Result<LoginNonce> {
    let nonce = LoginNonce::generate();
    let expires_at = chrono::Utc::now() + duration;

    sqlx::query!(
        "INSERT INTO pending_logins (nonce, device_code, interval_secs, created_at, expires_at) \
         VALUES ($1, $2, $3, now(), $4)",
        nonce.as_str(),
        device_code,
        interval_secs,
        expires_at,
    )
    .execute(&pool.0)
    .await
    .context("inserting pending login")?;

    Ok(nonce)
}

/// Look up a pending login by nonce.
/// Returns `None` if the nonce does not exist (consumed or never created).
/// The caller must check `expires_at` to determine if the login has expired.
pub async fn lookup_pending_login(
    pool: &DbPool,
    nonce: &LoginNonce,
) -> anyhow::Result<Option<PendingLoginRow>> {
    let row = sqlx::query_as!(
        RawPendingLoginRow,
        "SELECT device_code, interval_secs, expires_at \
         FROM pending_logins WHERE nonce = $1",
        nonce.as_str(),
    )
    .fetch_optional(&pool.0)
    .await
    .context("looking up pending login")?;

    Ok(row.map(RawPendingLoginRow::into))
}

/// Delete a pending login after successful completion or expiry.
pub async fn delete_pending_login(pool: &DbPool, nonce: &LoginNonce) -> anyhow::Result<()> {
    sqlx::query!(
        "DELETE FROM pending_logins WHERE nonce = $1",
        nonce.as_str(),
    )
    .execute(&pool.0)
    .await
    .context("deleting pending login")?;

    Ok(())
}

// ── Internal types ────────────────────────────────────────────────────────

struct RawPendingLoginRow {
    device_code: String,
    interval_secs: i32,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<RawPendingLoginRow> for PendingLoginRow {
    fn from(raw: RawPendingLoginRow) -> Self {
        Self {
            device_code: raw.device_code,
            interval_secs: raw.interval_secs,
            expires_at: raw.expires_at,
        }
    }
}

/// A looked-up pending login row.
pub struct PendingLoginRow {
    pub device_code: String,
    pub interval_secs: i32,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── LoginNonce ────────────────────────────────────────────────────────

    #[test]
    fn generate_produces_64_hex_chars() {
        let n = LoginNonce::generate();
        assert_eq!(n.as_str().len(), 64);
        assert!(n.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn from_raw_rejects_short() {
        assert!(LoginNonce::from_raw("abcd".to_string()).is_err());
    }

    #[test]
    fn from_raw_rejects_non_hex() {
        let s = "g".repeat(64);
        assert!(LoginNonce::from_raw(s).is_err());
    }

    #[test]
    fn from_raw_accepts_valid() {
        let s = "a1b2c3d4".repeat(8);
        assert!(LoginNonce::from_raw(s).is_ok());
    }

    // ── DB-backed tests ───────────────────────────────────────────────────

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_and_lookup_pending_login(pool: sqlx::PgPool) {
        let pool = DbPool(pool);

        let nonce = create_pending_login(
            &pool,
            "dev-code-xyz",
            5,
            std::time::Duration::from_secs(600),
        )
        .await
        .expect("create_pending_login failed");

        let row = lookup_pending_login(&pool, &nonce)
            .await
            .expect("lookup_pending_login failed")
            .expect("pending login not found");

        assert_eq!(row.device_code, "dev-code-xyz");
        assert_eq!(row.interval_secs, 5);
        assert!(row.expires_at > chrono::Utc::now());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn lookup_missing_nonce_returns_none(pool: sqlx::PgPool) {
        let pool = DbPool(pool);
        let bogus = LoginNonce::from_raw("a".repeat(64)).unwrap();

        let row = lookup_pending_login(&pool, &bogus)
            .await
            .expect("lookup_pending_login failed");

        assert!(row.is_none());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn delete_removes_pending_login(pool: sqlx::PgPool) {
        let pool = DbPool(pool);

        let nonce = create_pending_login(&pool, "dev-code", 5, std::time::Duration::from_secs(600))
            .await
            .expect("create_pending_login failed");

        delete_pending_login(&pool, &nonce)
            .await
            .expect("delete_pending_login failed");

        let row = lookup_pending_login(&pool, &nonce)
            .await
            .expect("lookup_pending_login failed");

        assert!(row.is_none());
    }
}
