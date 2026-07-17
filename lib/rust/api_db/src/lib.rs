mod admin;
mod db;
pub mod journal;
mod pending_login;
mod project;
mod role;
mod session;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Per-pool state this crate attaches to its connection pool: the activity flags its GC sweeps gate on.
/// A flag is set when a row of that kind is created or observed, and cleared by the matching sweep, so an idle instance's sweep issues no `DELETE`.
#[derive(Clone)]
pub struct PoolState {
    sessions_dirty: Arc<AtomicBool>,
    pending_logins_dirty: Arc<AtomicBool>,
}

impl Default for PoolState {
    /// Start dirty so the first sweep after process start runs.
    fn default() -> Self {
        Self {
            sessions_dirty: Arc::new(AtomicBool::new(true)),
            pending_logins_dirty: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl PoolState {
    pub(crate) fn mark_sessions(&self) {
        self.sessions_dirty.store(true, Ordering::Relaxed);
    }

    /// Clear the sessions flag, returning whether it was set.
    pub(crate) fn take_sessions_dirty(&self) -> bool {
        self.sessions_dirty.swap(false, Ordering::Relaxed)
    }

    pub(crate) fn mark_pending_logins(&self) {
        self.pending_logins_dirty.store(true, Ordering::Relaxed);
    }

    /// Clear the pending-logins flag, returning whether it was set.
    pub(crate) fn take_pending_logins_dirty(&self) -> bool {
        self.pending_logins_dirty.swap(false, Ordering::Relaxed)
    }
}

/// The connection pool as this crate uses it.
pub type Pool = db_pool::DbPool<PoolState>;

/// Re-export chrono types used in public structs (e.g. `SessionRow.expires_at`).
pub use sqlx::types::chrono;

pub use admin::{
    AuthProvider, DisplayName, Email, ServiceAccountId, Subject, UserId, create_admin, create_user,
    user_count,
};
pub use db::{instance_id, migrate};
pub use pending_login::{
    LoginNonce, PendingLoginRow, create_pending_login, delete_pending_login, gc_pending_logins,
    lookup_pending_login,
};
pub use project::{
    ProjectAccess, ProjectBatchStream, ProjectError, ProjectName, ProjectRow, ProjectVisibility,
    create_project, delete_project, find_project_by_name, get_project, list_projects,
    rename_project,
};
pub use role::{
    ProjectId, Role, RoleAssignment, assign_role, get_user_instance_roles, get_user_project_roles,
    get_user_roles,
};
pub use session::{
    SessionRow, SessionToken, UserRow, create_session, find_user_by_email, find_user_by_id,
    find_user_by_identity, gc_expired_sessions, lookup_session,
};
