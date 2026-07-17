//! Opaque pool handle. See README.md for the AI-guardrail rationale.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

mod sealed {
    pub trait Sealed {}
}

/// Installing connect options on a pool: the only capability the
/// refresh system grants a [`PoolRefresher`], so refreshers cannot
/// connect. The verb matches the sqlx method it delegates to.
pub trait SetConnectOptions {
    fn set_connect_options(&self, connect_opts: PgConnectOptions);
}

impl SetConnectOptions for sqlx::PgPool {
    fn set_connect_options(&self, connect_opts: PgConnectOptions) {
        sqlx::PgPool::set_connect_options(self, connect_opts);
    }
}

/// Closure that refreshes the pool's credentials in place. Supplied by
/// provider crates (e.g. `db_aws`) for IAM auth token rotation;
/// [`DbPool::start_background_refresh`] calls it with the pool it
/// holds.
pub type PoolRefresher = Arc<
    dyn for<'a> Fn(
            &'a (dyn SetConnectOptions + Sync),
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
        + Send
        + Sync,
>;

/// Opaque pool handle carrying a consumer-provided value `E` — a place
/// for a query crate to attach its own per-pool state, set at
/// construction and read via [`Self::state`]. Cloning the pool clones
/// `E`, so an `Arc`-backed `E` is shared across clones.
///
/// Static mode: a fixed pool, [`Self::start_background_refresh`] returns `None`.
///
/// Refreshing mode: a refresher closure is attached; the background
/// task periodically calls it to rotate the pool's credentials.
#[derive(Clone)]
pub struct DbPool<E> {
    inner: sqlx::PgPool,
    refresher: Option<PoolRefresher>,
    refresh_interval: Duration,
    state: E,
}

impl<E> sealed::Sealed for DbPool<E> {}

impl<E> DbPool<E> {
    /// Create a connection pool from a static database URL, attaching `state`.
    #[tracing::instrument(skip(database_url, state), fields(db.system = "postgresql"))]
    pub async fn connect(database_url: &str, state: E) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connecting to PostgreSQL")?;
        Ok(Self::from_pool(pool, state))
    }

    /// Wrap an existing pool with a refresher closure, attaching `state`.
    /// Provider crates use this to attach token-rotation logic (e.g.
    /// `db_aws::connect_iam`).
    pub fn from_pool_with_refresher(
        pool: sqlx::PgPool,
        refresh_interval: Duration,
        refresher: PoolRefresher,
        state: E,
    ) -> Self {
        Self {
            inner: pool,
            refresher: Some(refresher),
            refresh_interval,
            state,
        }
    }

    /// Wrap an existing pool with no refresher, attaching `state`. Use
    /// [`Self::connect`] for the static URL path; this is mainly for
    /// `#[sqlx::test]` fixtures and other scenarios where the pool already
    /// exists.
    pub fn from_pool(pool: sqlx::PgPool, state: E) -> Self {
        Self {
            inner: pool,
            refresher: None,
            refresh_interval: Duration::from_secs(6 * 3600),
            state,
        }
    }

    /// The attached state.
    pub fn state(&self) -> &E {
        &self.state
    }

    /// Spawn a background task that periodically rebuilds the pool via
    /// the refresher closure. Returns `None` for pools created via
    /// [`Self::connect`] / [`Self::from_pool`] (no refresher attached).
    pub fn start_background_refresh(&self) -> Option<tokio::task::JoinHandle<()>> {
        let refresher = self.refresher.clone()?;
        let pool = self.inner.clone();
        let interval_dur = self.refresh_interval;
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval_dur);
            // Skip the immediate first tick — the pool was just created.
            interval.tick().await;
            loop {
                interval.tick().await;
                match refresher(&pool).await {
                    Ok(()) => {
                        tracing::info!("database pool credentials refreshed");
                    }
                    Err(e) => {
                        tracing::warn!("database pool refresh failed: {e}");
                    }
                }
            }
        }))
    }
}

/// Sealed trait that exposes the underlying `sqlx::PgPool`. Only crates
/// that legitimately write SQL should `use db_pool::QueryAccess` to bring
/// these methods into scope. Business-logic crates that depend on
/// `db_connect` (which re-exports `DbPool` but not this trait) cannot
/// reach the pool.
pub trait QueryAccess: sealed::Sealed {
    /// Return the `sqlx::PgPool`. Cheap (internally Arc-counted).
    fn pool(&self) -> sqlx::PgPool;
}

impl<E> QueryAccess for DbPool<E> {
    fn pool(&self) -> sqlx::PgPool {
        self.inner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_pool_attaches_state_without_refresher() {
        // Sentinel pool — never connected — proves the construction path.
        let pool = sqlx::postgres::PgPool::connect_lazy("postgresql://invalid")
            .expect("lazy connect builds options");
        let db = DbPool::from_pool(pool, 7u32);

        assert_eq!(*db.state(), 7);
        // Cloning clones the state.
        assert_eq!(*db.clone().state(), 7);
        // No refresher was attached.
        assert!(db.start_background_refresh().is_none());
    }
}
