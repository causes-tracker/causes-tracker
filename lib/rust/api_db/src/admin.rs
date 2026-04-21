use anyhow::Context;
use uuid::Uuid;

use crate::db::DbPool;

// ── Parameter newtypes ─────────────────────────────────────────────────────

/// Display name for a user.
/// Must be non-empty and at most 200 characters.
#[derive(Debug, Clone)]
pub struct DisplayName(String);

impl DisplayName {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        anyhow::ensure!(!s.is_empty(), "DisplayName must not be empty");
        anyhow::ensure!(s.len() <= 200, "DisplayName must be at most 200 characters");
        anyhow::ensure!(
            !s.chars().any(|c| c.is_control()),
            "DisplayName must not contain control characters"
        );
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DisplayName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Email address, validated by `email_address` (RFC 5321/5322).
#[derive(Debug, Clone)]
pub struct Email(String);

impl Email {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        let parsed =
            email_address::EmailAddress::parse_with_options(&s, email_address::Options::default())
                .map_err(|e| anyhow::anyhow!("Email is not valid: {e}"))?;
        // Round-trip through the parser to canonicalise and prevent smuggling.
        Ok(Self(parsed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// OAuth issuer identifier.
/// Accepts a full URL (e.g. `https://accounts.google.com`) or a bare hostname
/// (e.g. `accounts.google.com`).
#[derive(Debug, Clone)]
pub struct AuthProvider(String);

impl AuthProvider {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        anyhow::ensure!(!s.is_empty(), "AuthProvider must not be empty");
        if s.contains("://") {
            let parsed = s
                .parse::<url::Url>()
                .context("AuthProvider is not a valid URL")?;
            // Round-trip to canonicalise and prevent smuggling.
            Ok(Self(parsed.to_string()))
        } else {
            // Bare hostname: validate by prepending a scheme and parsing.
            let parsed = format!("https://{s}")
                .parse::<url::Url>()
                .context("AuthProvider is not a valid hostname")?;
            // Store the canonicalised hostname (strip the scheme we added).
            Ok(Self(
                parsed
                    .host_str()
                    .context("no host in parsed URL")?
                    .to_owned(),
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// OAuth subject identifier.
/// Must be non-empty and at most 255 bytes (OIDC Core §8).
#[derive(Debug, Clone)]
pub struct Subject(String);

impl Subject {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        anyhow::ensure!(!s.is_empty(), "Subject must not be empty");
        anyhow::ensure!(
            s.len() <= 255,
            "Subject must be at most 255 bytes (OIDC Core §8)"
        );
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque user identifier (UUID v4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserId(String);

impl UserId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Parse and validate a user ID string (must be a valid UUID).
    pub fn from_raw(s: &str) -> anyhow::Result<Self> {
        s.parse::<Uuid>()
            .map_err(|e| anyhow::anyhow!("UserId must be a valid UUID: {e}"))?;
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Return the number of rows in the `users` table.
pub async fn user_count(pool: &DbPool) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(&pool.pool())
        .await
        .context("counting users")?
        .unwrap_or(0))
}

/// Insert a new instance-admin user and return the generated user id.
///
/// `auth_provider` is the issuer URL (e.g. "accounts.google.com").
pub async fn create_admin(
    pool: &DbPool,
    display_name: &DisplayName,
    email: &Email,
    auth_provider: &AuthProvider,
    subject: &Subject,
) -> anyhow::Result<UserId> {
    let user_id = UserId::new();
    let mut tx = pool.begin_txn().await?;

    sqlx::query!(
        "INSERT INTO users (id, display_name, email, auth_provider) VALUES ($1, $2, $3, $4)",
        user_id.as_str(),
        display_name.as_str(),
        email.as_str(),
        auth_provider.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("inserting user")?;

    sqlx::query!(
        "INSERT INTO external_identities (issuer, subject, user_id) VALUES ($1, $2, $3)",
        auth_provider.as_str(),
        subject.as_str(),
        user_id.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("inserting external_identity")?;

    sqlx::query!(
        "INSERT INTO role_assignments (user_id, role) VALUES ($1, 'instance-admin')",
        user_id.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("inserting role_assignment")?;

    tx.commit().await.context("committing transaction")?;
    Ok(user_id)
}

/// Insert a new user (no roles) and return the generated user id.
///
/// Unlike [`create_admin`], this does **not** insert any `role_assignments`.
/// The caller is responsible for granting roles separately.
pub async fn create_user(
    pool: &DbPool,
    display_name: &DisplayName,
    email: &Email,
    auth_provider: &AuthProvider,
    subject: &Subject,
) -> anyhow::Result<UserId> {
    let user_id = UserId::new();
    let mut tx = pool.begin_txn().await?;

    sqlx::query!(
        "INSERT INTO users (id, display_name, email, auth_provider) VALUES ($1, $2, $3, $4)",
        user_id.as_str(),
        display_name.as_str(),
        email.as_str(),
        auth_provider.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("inserting user")?;

    sqlx::query!(
        "INSERT INTO external_identities (issuer, subject, user_id) VALUES ($1, $2, $3)",
        auth_provider.as_str(),
        subject.as_str(),
        user_id.as_str(),
    )
    .execute(&mut *tx)
    .await
    .context("inserting external_identity")?;

    tx.commit().await.context("committing transaction")?;
    Ok(user_id)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── DisplayName ────────────────────────────────────────────────────────

    #[test]
    fn display_name_rejects_empty() {
        assert!(DisplayName::new("").is_err());
    }

    #[test]
    fn display_name_rejects_over_200_chars() {
        assert!(DisplayName::new("a".repeat(201)).is_err());
    }

    #[test]
    fn display_name_accepts_valid() {
        let d = DisplayName::new("Alice").unwrap();
        assert_eq!(d.as_str(), "Alice");
        assert_eq!(d.to_string(), "Alice");
    }

    #[test]
    fn display_name_rejects_newline() {
        assert!(DisplayName::new("Alice\nBob").is_err());
    }

    #[test]
    fn display_name_rejects_tab() {
        assert!(DisplayName::new("Alice\tBob").is_err());
    }

    #[test]
    fn display_name_rejects_carriage_return() {
        assert!(DisplayName::new("Alice\rBob").is_err());
    }

    #[test]
    fn display_name_rejects_null() {
        assert!(DisplayName::new("Alice\0Bob").is_err());
    }

    #[test]
    fn display_name_accepts_200_chars() {
        assert!(DisplayName::new("a".repeat(200)).is_ok());
    }

    // ── Email ──────────────────────────────────────────────────────────────

    #[test]
    fn email_rejects_empty() {
        assert!(Email::new("").is_err());
    }

    #[test]
    fn email_rejects_no_at() {
        assert!(Email::new("notanemail").is_err());
    }

    #[test]
    fn email_rejects_empty_local() {
        assert!(Email::new("@domain.com").is_err());
    }

    #[test]
    fn email_rejects_empty_domain() {
        assert!(Email::new("user@").is_err());
    }

    #[test]
    fn email_accepts_valid() {
        let e = Email::new("user@example.com").unwrap();
        assert_eq!(e.as_str(), "user@example.com");
    }

    // ── AuthProvider ───────────────────────────────────────────────────────

    #[test]
    fn auth_provider_rejects_empty() {
        assert!(AuthProvider::new("").is_err());
    }

    #[test]
    fn auth_provider_rejects_invalid_url() {
        assert!(AuthProvider::new("not a valid hostname!@#").is_err());
    }

    #[test]
    fn auth_provider_accepts_bare_hostname() {
        let a = AuthProvider::new("accounts.google.com").unwrap();
        assert_eq!(a.as_str(), "accounts.google.com");
    }

    #[test]
    fn auth_provider_accepts_full_url() {
        let a = AuthProvider::new("https://accounts.google.com").unwrap();
        // Round-tripped through url::Url — trailing slash is canonical form.
        assert_eq!(a.as_str(), "https://accounts.google.com/");
    }

    #[test]
    fn auth_provider_rejects_malformed_url() {
        assert!(AuthProvider::new("https://").is_err());
    }

    // ── Subject ────────────────────────────────────────────────────────────

    #[test]
    fn subject_rejects_empty() {
        assert!(Subject::new("").is_err());
    }

    #[test]
    fn subject_rejects_over_255_bytes() {
        assert!(Subject::new("a".repeat(256)).is_err());
    }

    #[test]
    fn subject_accepts_valid() {
        let s = Subject::new("sub-123").unwrap();
        assert_eq!(s.as_str(), "sub-123");
    }

    #[test]
    fn subject_accepts_255_bytes() {
        assert!(Subject::new("a".repeat(255)).is_ok());
    }

    // ── DB-backed tests (require DATABASE_URL) ─────────────────────────────

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_admin_inserts_rows(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);

        let user_id = create_admin(
            &pool,
            &DisplayName::new("Test Admin").unwrap(),
            &Email::new("admin@example.com").unwrap(),
            &AuthProvider::new("accounts.google.com").unwrap(),
            &Subject::new("test-sub-42").unwrap(),
        )
        .await
        .expect("create_admin failed");

        let ext_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM external_identities WHERE user_id = $1",
            user_id.as_str(),
        )
        .fetch_one(&pool.pool())
        .await
        .unwrap()
        .unwrap_or(0);
        assert_eq!(ext_count, 1);

        let role: String = sqlx::query_scalar!(
            "SELECT role FROM role_assignments WHERE user_id = $1",
            user_id.as_str()
        )
        .fetch_one(&pool.pool())
        .await
        .unwrap();
        assert_eq!(role, "instance-admin");
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn create_user_inserts_rows_without_role(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);

        let user_id = create_user(
            &pool,
            &DisplayName::new("New User").unwrap(),
            &Email::new("new@example.com").unwrap(),
            &AuthProvider::new("accounts.google.com").unwrap(),
            &Subject::new("new-sub-99").unwrap(),
        )
        .await
        .expect("create_user failed");

        let ext_count: Option<i64> = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM external_identities WHERE user_id = $1",
            user_id.as_str(),
        )
        .fetch_one(&pool.pool())
        .await
        .unwrap();
        assert_eq!(ext_count.unwrap_or(0), 1);

        let role = sqlx::query_scalar!(
            "SELECT role FROM role_assignments WHERE user_id = $1",
            user_id.as_str(),
        )
        .fetch_optional(&pool.pool())
        .await
        .unwrap();
        assert!(role.is_none(), "expected no role_assignments");
    }

    #[sqlx::test(migrator = "crate::db::MIGRATIONS")]
    async fn user_count_returns_nonnegative(pool: sqlx::PgPool) {
        let pool = DbPool::from_pool(pool);

        let count = user_count(&pool).await.expect("user_count failed");
        assert!(count >= 0);
    }
}
