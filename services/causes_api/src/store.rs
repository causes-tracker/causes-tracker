use std::future::Future;

/// Abstraction over database operations needed by this service.
/// Implemented by [`api_db::DbPool`] in production and by fakes in tests.
pub trait Store: Send + 'static {
    fn migrate(&self) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn user_count(&self) -> impl Future<Output = anyhow::Result<i64>> + Send;
    fn create_admin(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> impl Future<Output = anyhow::Result<api_db::UserId>> + Send;
}

impl Store for api_db::DbPool {
    fn migrate(&self) -> impl Future<Output = anyhow::Result<()>> + Send {
        api_db::DbPool::migrate(self)
    }

    fn user_count(&self) -> impl Future<Output = anyhow::Result<i64>> + Send {
        api_db::user_count(self)
    }

    fn create_admin(
        &self,
        display_name: &api_db::DisplayName,
        email: &api_db::Email,
        auth_provider: &api_db::AuthProvider,
        subject: &api_db::Subject,
    ) -> impl Future<Output = anyhow::Result<api_db::UserId>> + Send {
        let display_name = display_name.clone();
        let email = email.clone();
        let auth_provider = auth_provider.clone();
        let subject = subject.clone();
        let pool = self.clone();
        async move { api_db::create_admin(&pool, &display_name, &email, &auth_provider, &subject).await }
    }
}
