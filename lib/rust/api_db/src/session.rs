use anyhow::Context;
use sqlx::types::chrono;
use uuid::Uuid;

use crate::DbPool;
use crate::admin::{DisplayName, Email, UserId};

// ── Newtypes ──────────────────────────────────────────────────────────────

/// Opaque session token: 64 hex characters (32 random bytes).
///
/// Generated from two UUID v4s concatenated — each UUID v4 provides 122 bits
/// of CSPRNG entropy via `getrandom`, giving 244 bits total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generate a fresh random token.
    fn generate() -> Self {
        let a = Uuid::new_v4().simple().to_string();
        let b = Uuid::new_v4().simple().to_string();
        Self(format!("{a}{b}"))
    }

    /// Wrap an existing hex string (e.g. from the database).
    pub fn from_raw(s: String) -> anyhow::Result<Self> {
        anyhow::ensure!(s.len() == 64, "session token must be 64 hex characters");
        anyhow::ensure!(
            s.chars().all(|c| c.is_ascii_hexdigit()),
            "session token must contain only hex characters"
        );
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Public API ────────────────────────────────────────────────────────────

/// Create a new session for the given user, valid for `duration`.
///
/// When `restricted` is true, authorization helpers suppress elevated roles
/// (e.g. instance-admin) for this session.
pub async fn create_session(
    pool: &DbPool,
    user_id: &UserId,
    duration: std::time::Duration,
    restricted: bool,
) -> anyhow::Result<SessionToken> {
    let token = SessionToken::generate();
    let expires_at = chrono::Utc::now() + duration;

    sqlx::query!(
        "INSERT INTO sessions (token, user_id, created_at, expires_at, restricted) \
         VALUES ($1, $2, now(), $3, $4)",
        token.as_str(),
        user_id.as_str(),
        expires_at,
        restricted,
    )
    .execute(&pool.pool())
    .await
    .context("inserting session")?;

    Ok(token)
}

/// Look up a session by token. Returns `None` if the token does not exist.
/// The caller must check `expires_at` to determine if the session is still valid.
pub async fn lookup_session(
    pool: &DbPool,
    token: &SessionToken,
) -> anyhow::Result<Option<SessionRow>> {
    let row = sqlx::query_as!(
        RawSessionRow,
        "SELECT user_id, expires_at, restricted FROM sessions WHERE token = $1",
        token.as_str(),
    )
    .fetch_optional(&pool.pool())
    .await
    .context("looking up session")?;

    row.map(RawSessionRow::try_into).transpose()
}

/// Find a local user by their external identity (issuer + subject).
/// Returns `None` if the identity is not linked to any user.
pub async fn find_user_by_identity(
    pool: &DbPool,
    issuer: &str,
    subject: &str,
) -> anyhow::Result<Option<UserId>> {
    let row = sqlx::query_scalar!(
        "SELECT user_id FROM external_identities \
         WHERE issuer = $1 AND subject = $2",
        issuer,
        subject,
    )
    .fetch_optional(&pool.pool())
    .await
    .context("finding user by identity")?;

    row.map(|s| UserId::from_raw(&s)).transpose()
}

/// Fetch a user's display name and email by their id.
/// Returns `None` if the user does not exist.
pub async fn find_user_by_id(pool: &DbPool, user_id: &UserId) -> anyhow::Result<Option<UserRow>> {
    let row = sqlx::query_as!(
        RawUserRow,
        "SELECT display_name, email FROM users WHERE id = $1",
        user_id.as_str(),
    )
    .fetch_optional(&pool.pool())
    .await
    .context("finding user by id")?;

    row.map(RawUserRow::try_into).transpose()
}

/// Find a user by email address. Returns the user ID if exactly one user matches.
/// Returns `None` if no user has this email.
/// Errors if multiple users share the same email (ambiguous).
pub async fn find_user_by_email(pool: &DbPool, email: &str) -> anyhow::Result<Option<UserId>> {
    let rows = sqlx::query_scalar!("SELECT id FROM users WHERE email = $1", email,)
        .fetch_all(&pool.pool())
        .await
        .context("finding user by email")?;

    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(UserId::from_raw(&rows[0])?)),
        n => anyhow::bail!("ambiguous: {n} users share email {email:?}"),
    }
}

/// Delete expired sessions.
/// Called periodically to garbage-collect sessions past their `expires_at`.
pub async fn gc_expired_sessions(pool: &DbPool) -> anyhow::Result<u64> {
    let result = sqlx::query!("DELETE FROM sessions WHERE expires_at < now()")
        .execute(&pool.pool())
        .await
        .context("garbage-collecting expired sessions")?;

    Ok(result.rows_affected())
}

// ── Internal types ────────────────────────────────────────────────────────

struct RawSessionRow {
    user_id: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    restricted: bool,
}

impl RawSessionRow {
    fn try_into(self) -> anyhow::Result<SessionRow> {
        Ok(SessionRow {
            user_id: UserId::from_raw(&self.user_id)?,
            expires_at: self.expires_at,
            restricted: self.restricted,
        })
    }
}

/// A looked-up session row.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub user_id: UserId,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// When true, elevated roles (e.g. instance-admin) are suppressed.
    pub restricted: bool,
}

impl SessionRow {
    /// Returns `true` if this session has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at < chrono::Utc::now()
    }
}

struct RawUserRow {
    display_name: String,
    email: String,
}

impl RawUserRow {
    fn try_into(self) -> anyhow::Result<UserRow> {
        Ok(UserRow {
            display_name: DisplayName::new(self.display_name)?,
            email: Email::new(self.email)?,
        })
    }
}

/// A looked-up user record.
pub struct UserRow {
    pub display_name: DisplayName,
    pub email: Email,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{AuthProvider, Subject, create_admin};

    // ── SessionToken ──────────────────────────────────────────────────────

    #[test]
    fn generate_produces_64_hex_chars() {
        let t = SessionToken::generate();
        assert_eq!(t.as_str().len(), 64);
        assert!(t.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn from_raw_rejects_short() {
        assert!(SessionToken::from_raw("abcd".to_string()).is_err());
    }

    #[test]
    fn from_raw_rejects_non_hex() {
        let s = "g".repeat(64);
        assert!(SessionToken::from_raw(s).is_err());
    }

    #[test]
    fn from_raw_accepts_valid() {
        let s = "a1b2c3d4".repeat(8);
        assert!(SessionToken::from_raw(s).is_ok());
    }

    // ── DB-backed tests ───────────────────────────────────────────────────

    /// Helper: create a test admin and return their UserId.
    async fn seed_admin(pool: &DbPool) -> UserId {
        create_admin(
            pool,
            &DisplayName::new("Test Admin").unwrap(),
            &Email::new("admin@example.com").unwrap(),
            &AuthProvider::new("accounts.google.com").unwrap(),
            &Subject::new("test-sub-42").unwrap(),
        )
        .await
        .expect("seed_admin failed")
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_and_lookup_session(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);
        let user_id = seed_admin(&pool).await;

        let token = create_session(&pool, &user_id, std::time::Duration::from_secs(3600), true)
            .await
            .expect("create_session failed");

        let row = lookup_session(&pool, &token)
            .await
            .expect("lookup_session failed")
            .expect("session not found");

        assert_eq!(row.user_id, user_id);
        assert!(row.expires_at > chrono::Utc::now());
        assert!(row.restricted);
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_unrestricted_session(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);
        let user_id = seed_admin(&pool).await;

        let token = create_session(&pool, &user_id, std::time::Duration::from_secs(3600), false)
            .await
            .expect("create_session failed");

        let row = lookup_session(&pool, &token)
            .await
            .expect("lookup_session failed")
            .expect("session not found");

        assert_eq!(row.user_id, user_id);
        assert!(!row.restricted);
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn lookup_missing_token_returns_none(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);
        let bogus = SessionToken::from_raw("a".repeat(64)).unwrap();

        let row = lookup_session(&pool, &bogus)
            .await
            .expect("lookup_session failed");

        assert!(row.is_none());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn find_user_by_identity_returns_match(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);
        let user_id = seed_admin(&pool).await;

        let found = find_user_by_identity(&pool, "accounts.google.com", "test-sub-42")
            .await
            .expect("find_user_by_identity failed");

        assert_eq!(found, Some(user_id));
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn find_user_by_identity_returns_none_for_unknown(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);

        let found = find_user_by_identity(&pool, "unknown.issuer", "no-such-sub")
            .await
            .expect("find_user_by_identity failed");

        assert!(found.is_none());
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn find_user_by_id_returns_match(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);
        let user_id = seed_admin(&pool).await;

        let row = find_user_by_id(&pool, &user_id)
            .await
            .expect("find_user_by_id failed")
            .expect("user not found");

        assert_eq!(row.display_name.as_str(), "Test Admin");
        assert_eq!(row.email.as_str(), "admin@example.com");
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn find_user_by_id_returns_none_for_unknown(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);
        let bogus = UserId::new();

        let row = find_user_by_id(&pool, &bogus)
            .await
            .expect("find_user_by_id failed");

        assert!(row.is_none());
    }
}
