mod admin;
mod db;
mod pending_login;
mod project;
mod role;
mod session;

/// Re-export chrono types used in public structs (e.g. `SessionRow.expires_at`).
pub use sqlx::types::chrono;

pub use admin::{
    AuthProvider, DisplayName, Email, Subject, UserId, create_admin, create_user, user_count,
};
pub use db::DbPool;
pub use pending_login::{
    LoginNonce, PendingLoginRow, create_pending_login, delete_pending_login, gc_pending_logins,
    lookup_pending_login,
};
pub use project::{
    ProjectName, ProjectRow, ProjectVisibility, create_project, delete_project, get_project,
    list_projects, rename_project,
};
pub use role::{
    ProjectId, Role, RoleAssignment, assign_role, get_user_instance_roles, get_user_project_roles,
    get_user_roles,
};
pub use session::{
    SessionRow, SessionToken, UserRow, create_session, find_user_by_id, find_user_by_identity,
    gc_expired_sessions, lookup_session,
};
