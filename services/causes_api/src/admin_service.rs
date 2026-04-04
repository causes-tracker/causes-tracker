use std::sync::Arc;

use tonic::{Request, Response, Status};

use causes_proto::admin_service_server::AdminService;
use causes_proto::{GrantRoleRequest, GrantRoleResponse};

fn proto_role_to_db(role: causes_proto::Role) -> Result<api_db::Role, Status> {
    match role {
        causes_proto::Role::InstanceAdmin => Ok(api_db::Role::InstanceAdmin),
        causes_proto::Role::Developer => Ok(api_db::Role::Developer),
        causes_proto::Role::ProjectMaintainer => Ok(api_db::Role::ProjectMaintainer),
        causes_proto::Role::SecurityTeam => Ok(api_db::Role::SecurityTeam),
        causes_proto::Role::Unspecified => Err(Status::invalid_argument("role must be specified")),
    }
}

pub struct AdminHandler<S> {
    store: Arc<S>,
}

impl<S: crate::store::Store> AdminHandler<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
}

#[tonic::async_trait]
impl<S: crate::store::Store> AdminService for AdminHandler<S> {
    #[tracing::instrument(skip(self, request))]
    async fn grant_role(
        &self,
        request: Request<GrantRoleRequest>,
    ) -> Result<Response<GrantRoleResponse>, Status> {
        crate::interceptor::authorize_instance_role(
            &self.store,
            request.metadata(),
            api_db::Role::InstanceAdmin,
        )
        .await?;

        let req = request.into_inner();

        let proto_role = causes_proto::Role::try_from(req.role)
            .map_err(|_| Status::invalid_argument("invalid role value"))?;
        let role = proto_role_to_db(proto_role)?;

        let user_id = self
            .store
            .find_user_by_email(&req.email)
            .await
            .map_err(|e| Status::internal(format!("looking up user: {e}")))?
            .ok_or_else(|| Status::not_found("no user with that email"))?;

        let project_id = if req.project.is_empty() {
            None
        } else {
            Some(
                self.store
                    .find_project_id_by_name(&req.project)
                    .await
                    .map_err(|e| Status::internal(format!("looking up project: {e}")))?
                    .ok_or_else(|| Status::not_found("no project with that name"))?,
            )
        };

        self.store
            .assign_role(&user_id, &project_id, role)
            .await
            .map_err(|e| Status::internal(format!("assigning role: {e}")))?;

        Ok(Response::new(GrantRoleResponse {}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MockStore;
    use tonic::metadata::MetadataValue;

    fn grant_request(token: &str, email: &str, role: i32) -> Request<GrantRoleRequest> {
        let mut req = Request::new(GrantRoleRequest {
            email: email.to_owned(),
            role,
            project: String::new(),
        });
        req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).unwrap(),
        );
        req
    }

    fn mock_admin_session(user_id: api_db::UserId) -> MockStore {
        let uid = user_id.clone();
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(move |_| {
            Ok(Some(api_db::SessionRow {
                user_id: uid.clone(),
                expires_at: api_db::chrono::Utc::now() + std::time::Duration::from_secs(3600),
                restricted: false,
            }))
        });
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![api_db::Role::InstanceAdmin]));
        store
    }

    #[tokio::test]
    async fn grant_role_succeeds_for_admin() {
        let admin_id = api_db::UserId::new();
        let target_id = api_db::UserId::new();
        let mut store = mock_admin_session(admin_id);
        store
            .expect_find_user_by_email()
            .returning(move |_| Ok(Some(target_id.clone())));
        store.expect_assign_role().returning(|_, _, _| Ok(()));

        let handler = AdminHandler::new(Arc::new(store));
        let resp = handler
            .grant_role(grant_request(
                &"a".repeat(64),
                "target@example.com",
                causes_proto::Role::Developer.into(),
            ))
            .await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn grant_role_rejects_non_admin() {
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
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![]));

        let handler = AdminHandler::new(Arc::new(store));
        let err = handler
            .grant_role(grant_request(
                &"a".repeat(64),
                "user@example.com",
                causes_proto::Role::Developer.into(),
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn grant_role_rejects_unknown_role() {
        let admin_id = api_db::UserId::new();
        let store = mock_admin_session(admin_id);

        let handler = AdminHandler::new(Arc::new(store));
        let err = handler
            .grant_role(grant_request(&"a".repeat(64), "user@example.com", 99))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn grant_role_rejects_unknown_user() {
        let admin_id = api_db::UserId::new();
        let mut store = mock_admin_session(admin_id);
        store.expect_find_user_by_email().returning(|_| Ok(None));

        let handler = AdminHandler::new(Arc::new(store));
        let err = handler
            .grant_role(grant_request(
                &"a".repeat(64),
                "nobody@example.com",
                causes_proto::Role::Developer.into(),
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn grant_role_rejects_unauthenticated() {
        let store = MockStore::new();
        let handler = AdminHandler::new(Arc::new(store));
        let req = Request::new(GrantRoleRequest {
            email: "user@example.com".to_owned(),
            role: causes_proto::Role::Developer.into(),
            project: String::new(),
        });
        let err = handler.grant_role(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }
}
