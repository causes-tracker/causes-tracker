use std::sync::Arc;

use tonic::Status;

use crate::store::Store;

/// Extract the bearer token from request metadata, validate the session,
/// and return the session row.
///
/// Called explicitly by authenticated RPC handlers. Unauthenticated RPCs
/// (StartLogin, CompleteLogin, health) simply don't call this.
pub async fn authenticate<S: Store>(
    store: &Arc<S>,
    metadata: &tonic::metadata::MetadataMap,
) -> Result<api_db::SessionRow, Status> {
    let token_str = metadata
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("missing authorization header"))?
        .to_str()
        .map_err(|_| Status::unauthenticated("authorization header is not valid ASCII"))?;

    let token_hex = token_str
        .strip_prefix("Bearer ")
        .ok_or_else(|| Status::unauthenticated("authorization header must start with 'Bearer '"))?;

    let token = api_db::SessionToken::from_raw(token_hex.to_owned())
        .map_err(|e| Status::unauthenticated(format!("invalid session token: {e}")))?;

    let session = store
        .lookup_session(&token)
        .await
        .map_err(|e| Status::internal(format!("session lookup failed: {e}")))?
        .ok_or_else(|| Status::unauthenticated("unknown session token"))?;

    if session.is_expired() {
        return Err(Status::unauthenticated("session has expired"));
    }

    Ok(session)
}

/// Check whether a role list satisfies a required role.
///
/// `InstanceAdmin` acts as superuser (satisfies any check), unless the
/// session is restricted — restricted sessions suppress `InstanceAdmin`.
pub(crate) fn has_required_role(
    roles: &[api_db::Role],
    required: api_db::Role,
    restricted: bool,
) -> bool {
    roles.iter().any(|&role| {
        if role == api_db::Role::InstanceAdmin && restricted {
            return false;
        }
        role == required || role == api_db::Role::InstanceAdmin
    })
}

#[allow(dead_code)]
/// Authenticate the caller and verify they hold the given role at instance level.
pub async fn authorize_instance_role<S: Store>(
    store: &Arc<S>,
    metadata: &tonic::metadata::MetadataMap,
    required_role: api_db::Role,
) -> Result<api_db::UserId, Status> {
    let session = authenticate(store, metadata).await?;

    let roles = store
        .get_user_instance_roles(&session.user_id)
        .await
        .map_err(|e| Status::internal(format!("querying roles: {e}")))?;

    if has_required_role(&roles, required_role, session.restricted) {
        Ok(session.user_id)
    } else {
        Err(Status::permission_denied("insufficient permissions"))
    }
}

#[allow(dead_code)]
/// Authenticate the caller and verify they hold the given role for a project.
///
/// Includes instance-level roles (e.g. instance-admin satisfies any project check).
pub async fn authorize_project_role<S: Store>(
    store: &Arc<S>,
    metadata: &tonic::metadata::MetadataMap,
    project_id: &api_db::ProjectId,
    required_role: api_db::Role,
) -> Result<api_db::UserId, Status> {
    let session = authenticate(store, metadata).await?;

    let roles = store
        .get_user_project_roles(&session.user_id, project_id)
        .await
        .map_err(|e| Status::internal(format!("querying project roles: {e}")))?;

    if has_required_role(&roles, required_role, session.restricted) {
        Ok(session.user_id)
    } else {
        Err(Status::permission_denied("insufficient permissions"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MockStore;
    use tonic::metadata::MetadataValue;

    fn metadata_with_bearer(token: &str) -> tonic::metadata::MetadataMap {
        let mut md = tonic::metadata::MetadataMap::new();
        md.insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).unwrap(),
        );
        md
    }

    #[tokio::test]
    async fn rejects_missing_authorization() {
        let store = Arc::new(MockStore::new());
        let md = tonic::metadata::MetadataMap::new();
        let err = authenticate(&store, &md).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("missing"));
    }

    #[tokio::test]
    async fn rejects_non_bearer_scheme() {
        let store = Arc::new(MockStore::new());
        let mut md = tonic::metadata::MetadataMap::new();
        md.insert(
            "authorization",
            MetadataValue::try_from("Basic dXNlcjpwYXNz").unwrap(),
        );
        let err = authenticate(&store, &md).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("Bearer"));
    }

    #[tokio::test]
    async fn rejects_invalid_token_format() {
        let store = Arc::new(MockStore::new());
        let md = metadata_with_bearer("too-short");
        let err = authenticate(&store, &md).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn rejects_unknown_token() {
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(|_| Ok(None));
        let store = Arc::new(store);

        let md = metadata_with_bearer(&"a".repeat(64));
        let err = authenticate(&store, &md).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("unknown"));
    }

    #[tokio::test]
    async fn rejects_expired_session() {
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(|_| {
            Ok(Some(api_db::SessionRow {
                user_id: api_db::UserId::new(),
                expires_at: api_db::chrono::Utc::now() - std::time::Duration::from_secs(1),
                restricted: true,
            }))
        });
        let store = Arc::new(store);

        let md = metadata_with_bearer(&"b".repeat(64));
        let err = authenticate(&store, &md).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("expired"));
    }

    #[tokio::test]
    async fn valid_token_returns_caller() {
        let user_id = api_db::UserId::new();
        let uid = user_id.clone();
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(move |_| {
            Ok(Some(api_db::SessionRow {
                user_id: uid.clone(),
                expires_at: api_db::chrono::Utc::now() + std::time::Duration::from_secs(3600),
                restricted: true,
            }))
        });
        let store = Arc::new(store);

        let md = metadata_with_bearer(&"c".repeat(64));
        let session = authenticate(&store, &md).await.unwrap();
        assert_eq!(session.user_id, user_id);
        assert!(session.restricted);
    }

    // ── authorize_instance_role tests ────────────────────────────────

    fn mock_store_with_session(
        user_id: api_db::UserId,
        restricted: bool,
    ) -> (api_db::UserId, MockStore) {
        let uid = user_id.clone();
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(move |_| {
            Ok(Some(api_db::SessionRow {
                user_id: uid.clone(),
                expires_at: api_db::chrono::Utc::now() + std::time::Duration::from_secs(3600),
                restricted,
            }))
        });
        (user_id, store)
    }

    #[tokio::test]
    async fn authorize_instance_role_succeeds_when_role_present() {
        let (user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![api_db::Role::Developer]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let result = authorize_instance_role(&store, &md, api_db::Role::Developer).await;
        assert_eq!(result.unwrap(), user_id);
    }

    #[tokio::test]
    async fn authorize_instance_role_admin_satisfies_any_role() {
        let (user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![api_db::Role::InstanceAdmin]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let result = authorize_instance_role(&store, &md, api_db::Role::Developer).await;
        assert_eq!(result.unwrap(), user_id);
    }

    #[tokio::test]
    async fn authorize_instance_role_rejects_missing_role() {
        let (_user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let err = authorize_instance_role(&store, &md, api_db::Role::Developer)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn authorize_instance_role_rejects_unauthenticated() {
        let store = Arc::new(MockStore::new());
        let md = tonic::metadata::MetadataMap::new();
        let err = authorize_instance_role(&store, &md, api_db::Role::Developer)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn authorize_instance_role_suppresses_admin_when_restricted() {
        let (_user_id, mut store) = mock_store_with_session(api_db::UserId::new(), true);
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![api_db::Role::InstanceAdmin]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let err = authorize_instance_role(&store, &md, api_db::Role::InstanceAdmin)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn authorize_instance_role_allows_admin_when_unrestricted() {
        let (user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![api_db::Role::InstanceAdmin]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let result = authorize_instance_role(&store, &md, api_db::Role::InstanceAdmin).await;
        assert_eq!(result.unwrap(), user_id);
    }

    // ── authorize_project_role tests ─────────────────────────────────

    #[tokio::test]
    async fn authorize_project_role_succeeds_when_role_present() {
        let (user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![api_db::Role::ProjectMaintainer]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let pid = api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
        let result =
            authorize_project_role(&store, &md, &pid, api_db::Role::ProjectMaintainer).await;
        assert_eq!(result.unwrap(), user_id);
    }

    #[tokio::test]
    async fn authorize_project_role_admin_satisfies_any_role() {
        let (user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![api_db::Role::InstanceAdmin]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let pid = api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
        let result =
            authorize_project_role(&store, &md, &pid, api_db::Role::ProjectMaintainer).await;
        assert_eq!(result.unwrap(), user_id);
    }

    #[tokio::test]
    async fn authorize_project_role_rejects_missing_role() {
        let (_user_id, mut store) = mock_store_with_session(api_db::UserId::new(), false);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let pid = api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
        let err = authorize_project_role(&store, &md, &pid, api_db::Role::ProjectMaintainer)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn authorize_project_role_rejects_unauthenticated() {
        let store = Arc::new(MockStore::new());
        let md = tonic::metadata::MetadataMap::new();
        let pid = api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
        let err = authorize_project_role(&store, &md, &pid, api_db::Role::ProjectMaintainer)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn authorize_project_role_suppresses_admin_when_restricted() {
        let (_user_id, mut store) = mock_store_with_session(api_db::UserId::new(), true);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![api_db::Role::InstanceAdmin]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let pid = api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
        let err = authorize_project_role(&store, &md, &pid, api_db::Role::ProjectMaintainer)
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn authorize_project_role_allows_non_admin_role_when_restricted() {
        let (user_id, mut store) = mock_store_with_session(api_db::UserId::new(), true);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![api_db::Role::ProjectMaintainer]));
        let store = Arc::new(store);
        let md = metadata_with_bearer(&"a".repeat(64));
        let pid = api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
        let result =
            authorize_project_role(&store, &md, &pid, api_db::Role::ProjectMaintainer).await;
        assert_eq!(result.unwrap(), user_id);
    }

    // ── authenticate tests (continued) ───────────────────────────────

    #[tokio::test]
    async fn unrestricted_session_returns_restricted_false() {
        let user_id = api_db::UserId::new();
        let uid = user_id.clone();
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(move |_| {
            Ok(Some(api_db::SessionRow {
                user_id: uid.clone(),
                expires_at: api_db::chrono::Utc::now() + std::time::Duration::from_secs(3600),
                restricted: false,
            }))
        });
        let store = Arc::new(store);

        let md = metadata_with_bearer(&"d".repeat(64));
        let session = authenticate(&store, &md).await.unwrap();
        assert_eq!(session.user_id, user_id);
        assert!(!session.restricted);
    }
}
