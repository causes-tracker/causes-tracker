/// Abstraction over database operations needed by this service.
/// Implemented by [`api_db::DbPool`] in production; in tests, use
/// [`mockall::automock`]-generated `MockStore`.
#[cfg_attr(test, mockall::automock)]
#[tonic::async_trait]
pub trait Store: Send + Sync + 'static {
    async fn migrate(&self) -> anyhow::Result<()>;
    async fn user_count(&self) -> anyhow::Result<i64>;
    async fn create_admin(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> anyhow::Result<api_db::UserId>;
    async fn create_user(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> anyhow::Result<api_db::UserId>;
    async fn create_session(
        &self,
        user_id: &api_db::UserId,
        duration: std::time::Duration,
        restricted: bool,
    ) -> anyhow::Result<api_db::SessionToken>;
    async fn lookup_session(
        &self,
        token: &api_db::SessionToken,
    ) -> anyhow::Result<Option<api_db::SessionRow>>;
    async fn find_user_by_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> anyhow::Result<Option<api_db::UserId>>;
    async fn find_user_by_id(
        &self,
        user_id: &api_db::UserId,
    ) -> anyhow::Result<Option<api_db::UserRow>>;
    async fn create_pending_login(
        &self,
        device_code: &str,
        interval_secs: i32,
    ) -> anyhow::Result<api_db::LoginNonce>;
    async fn lookup_pending_login(
        &self,
        nonce: &api_db::LoginNonce,
    ) -> anyhow::Result<Option<api_db::PendingLoginRow>>;
    async fn delete_pending_login(&self, nonce: &api_db::LoginNonce) -> anyhow::Result<()>;
    async fn gc_pending_logins(&self, max_age: std::time::Duration) -> anyhow::Result<u64>;
    async fn gc_expired_sessions(&self) -> anyhow::Result<u64>;
}

#[tonic::async_trait]
impl Store for api_db::DbPool {
    async fn migrate(&self) -> anyhow::Result<()> {
        api_db::DbPool::migrate(self).await
    }

    async fn user_count(&self) -> anyhow::Result<i64> {
        api_db::user_count(self).await
    }

    async fn create_admin(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> anyhow::Result<api_db::UserId> {
        api_db::create_admin(self, display_name, email, auth_provider, subject).await
    }

    async fn create_user(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> anyhow::Result<api_db::UserId> {
        api_db::create_user(self, display_name, email, auth_provider, subject).await
    }

    async fn create_session(
        &self,
        user_id: &api_db::UserId,
        duration: std::time::Duration,
        restricted: bool,
    ) -> anyhow::Result<api_db::SessionToken> {
        api_db::create_session(self, user_id, duration, restricted).await
    }

    async fn lookup_session(
        &self,
        token: &api_db::SessionToken,
    ) -> anyhow::Result<Option<api_db::SessionRow>> {
        api_db::lookup_session(self, token).await
    }

    async fn find_user_by_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> anyhow::Result<Option<api_db::UserId>> {
        api_db::find_user_by_identity(self, issuer, subject).await
    }

    async fn find_user_by_id(
        &self,
        user_id: &api_db::UserId,
    ) -> anyhow::Result<Option<api_db::UserRow>> {
        api_db::find_user_by_id(self, user_id).await
    }

    async fn create_pending_login(
        &self,
        device_code: &str,
        interval_secs: i32,
    ) -> anyhow::Result<api_db::LoginNonce> {
        api_db::create_pending_login(self, device_code, interval_secs).await
    }

    async fn lookup_pending_login(
        &self,
        nonce: &api_db::LoginNonce,
    ) -> anyhow::Result<Option<api_db::PendingLoginRow>> {
        api_db::lookup_pending_login(self, nonce).await
    }

    async fn delete_pending_login(&self, nonce: &api_db::LoginNonce) -> anyhow::Result<()> {
        api_db::delete_pending_login(self, nonce).await
    }

    async fn gc_pending_logins(&self, max_age: std::time::Duration) -> anyhow::Result<u64> {
        api_db::gc_pending_logins(self, max_age).await
    }

    async fn gc_expired_sessions(&self) -> anyhow::Result<u64> {
        api_db::gc_expired_sessions(self).await
    }
}
