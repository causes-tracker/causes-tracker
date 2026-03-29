/// Abstraction over database operations needed by this service.
/// Implemented by [`api_db::DbPool`] in production; in tests, use
/// [`mockall::automock`]-generated `MockStore`.
#[cfg_attr(test, mockall::automock)]
pub trait Store: Send + 'static {
    async fn migrate(&self) -> anyhow::Result<()>;
    async fn user_count(&self) -> anyhow::Result<i64>;
    async fn create_admin(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> anyhow::Result<api_db::UserId>;
}

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
}
