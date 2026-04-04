use std::sync::Arc;

use tonic::{Request, Response, Status};

use causes_proto::project_service_server::ProjectService;
use causes_proto::{
    CreateProjectRequest, CreateProjectResponse, DeleteProjectRequest, DeleteProjectResponse,
    GetProjectRequest, GetProjectResponse, ListProjectsRequest, ListProjectsResponse,
    RenameProjectRequest, RenameProjectResponse,
};

pub struct ProjectHandler<S> {
    store: Arc<S>,
}

impl<S: crate::store::Store> ProjectHandler<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }
}

fn project_row_to_proto(row: api_db::ProjectRow) -> causes_proto::Project {
    causes_proto::Project {
        id: row.id.as_str().to_owned(),
        name: row.name.as_str().to_owned(),
        description: row.description,
        visibility: match row.visibility {
            api_db::ProjectVisibility::Public => causes_proto::project::Visibility::Public.into(),
            api_db::ProjectVisibility::Private => causes_proto::project::Visibility::Private.into(),
        },
        embargoed_by_default: row.embargoed_by_default,
        created_at: Some(prost_types::Timestamp {
            seconds: row.created_at.timestamp(),
            nanos: row.created_at.timestamp_subsec_nanos() as i32,
        }),
    }
}

fn parse_visibility(v: i32) -> Result<api_db::ProjectVisibility, Status> {
    match causes_proto::project::Visibility::try_from(v) {
        Ok(causes_proto::project::Visibility::Public) => Ok(api_db::ProjectVisibility::Public),
        Ok(causes_proto::project::Visibility::Private) => Ok(api_db::ProjectVisibility::Private),
        _ => Err(Status::invalid_argument("visibility must be specified")),
    }
}

#[tonic::async_trait]
impl<S: crate::store::Store> ProjectService for ProjectHandler<S> {
    type ListProjectsStream = tokio_stream::Once<Result<ListProjectsResponse, Status>>;

    #[tracing::instrument(skip(self, request))]
    async fn create_project(
        &self,
        request: Request<CreateProjectRequest>,
    ) -> Result<Response<CreateProjectResponse>, Status> {
        let session = crate::interceptor::authenticate(&self.store, request.metadata()).await?;

        let roles = self
            .store
            .get_user_instance_roles(&session.user_id)
            .await
            .map_err(|e| Status::internal(format!("querying roles: {e}")))?;

        if !crate::interceptor::has_required_role(
            &roles,
            api_db::Role::Developer,
            session.restricted,
        ) {
            return Err(Status::permission_denied("insufficient permissions"));
        }

        let req = request.into_inner();

        let name = api_db::ProjectName::new(&req.name)
            .map_err(|e| Status::invalid_argument(format!("invalid project name: {e}")))?;
        let visibility = parse_visibility(req.visibility)?;

        let row = self
            .store
            .create_project(
                &name,
                &req.description,
                visibility,
                req.embargoed_by_default,
                &session.user_id,
            )
            .await
            .map_err(|e| match e {
                api_db::ProjectError::NameAlreadyExists => {
                    Status::already_exists("a project with that name already exists")
                }
                api_db::ProjectError::Other(e) => {
                    Status::internal(format!("creating project: {e}"))
                }
            })?;

        Ok(Response::new(CreateProjectResponse {
            project: Some(project_row_to_proto(row)),
        }))
    }

    #[tracing::instrument(skip(self, request))]
    async fn get_project(
        &self,
        request: Request<GetProjectRequest>,
    ) -> Result<Response<GetProjectResponse>, Status> {
        let session = crate::interceptor::authenticate(&self.store, request.metadata()).await?;

        let project_id = api_db::ProjectId::new(&request.into_inner().project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        let row = match self
            .store
            .get_project(&project_id, &session)
            .await
            .map_err(|e| Status::internal(format!("fetching project: {e}")))?
        {
            api_db::ProjectAccess::Visible(row) => row,
            api_db::ProjectAccess::AccessDenied => {
                return Err(Status::permission_denied("project access denied"));
            }
            api_db::ProjectAccess::NotFound => {
                return Err(Status::not_found("project not found"));
            }
        };

        Ok(Response::new(GetProjectResponse {
            project: Some(project_row_to_proto(row)),
        }))
    }

    #[tracing::instrument(skip(self, request))]
    async fn list_projects(
        &self,
        request: Request<ListProjectsRequest>,
    ) -> Result<Response<Self::ListProjectsStream>, Status> {
        let session = crate::interceptor::authenticate(&self.store, request.metadata()).await?;

        let rows = self
            .store
            .list_projects(&session)
            .await
            .map_err(|e| Status::internal(format!("listing projects: {e}")))?;

        let response = ListProjectsResponse {
            projects: rows.into_iter().map(project_row_to_proto).collect(),
        };
        Ok(Response::new(tokio_stream::once(Ok(response))))
    }

    #[tracing::instrument(skip(self, request))]
    async fn rename_project(
        &self,
        request: Request<RenameProjectRequest>,
    ) -> Result<Response<RenameProjectResponse>, Status> {
        let req = request.get_ref();
        let project_id = api_db::ProjectId::new(&req.project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        let session = crate::interceptor::authenticate(&self.store, request.metadata()).await?;

        let roles = self
            .store
            .get_user_project_roles(&session.user_id, &project_id)
            .await
            .map_err(|e| Status::internal(format!("querying roles: {e}")))?;

        if !crate::interceptor::has_required_role(
            &roles,
            api_db::Role::ProjectMaintainer,
            session.restricted,
        ) {
            return Err(Status::permission_denied("insufficient permissions"));
        }

        let new_name = api_db::ProjectName::new(&request.into_inner().new_name)
            .map_err(|e| Status::invalid_argument(format!("invalid project name: {e}")))?;

        let row = self
            .store
            .rename_project(&project_id, &new_name)
            .await
            .map_err(|e| match e {
                api_db::ProjectError::NameAlreadyExists => {
                    Status::already_exists("a project with that name already exists")
                }
                api_db::ProjectError::Other(e) => {
                    Status::internal(format!("renaming project: {e}"))
                }
            })?
            .ok_or_else(|| Status::not_found("project not found"))?;

        Ok(Response::new(RenameProjectResponse {
            project: Some(project_row_to_proto(row)),
        }))
    }

    #[tracing::instrument(skip(self, request))]
    async fn delete_project(
        &self,
        request: Request<DeleteProjectRequest>,
    ) -> Result<Response<DeleteProjectResponse>, Status> {
        let project_id = api_db::ProjectId::new(&request.get_ref().project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        let session = crate::interceptor::authenticate(&self.store, request.metadata()).await?;

        let roles = self
            .store
            .get_user_project_roles(&session.user_id, &project_id)
            .await
            .map_err(|e| Status::internal(format!("querying roles: {e}")))?;

        if !crate::interceptor::has_required_role(
            &roles,
            api_db::Role::ProjectMaintainer,
            session.restricted,
        ) {
            return Err(Status::permission_denied("insufficient permissions"));
        }

        let deleted = self
            .store
            .delete_project(&project_id)
            .await
            .map_err(|e| Status::internal(format!("deleting project: {e}")))?;

        if !deleted {
            return Err(Status::not_found("project not found"));
        }

        Ok(Response::new(DeleteProjectResponse {}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MockStore;
    use tonic::metadata::MetadataValue;

    fn authed_request<T>(token: &str, inner: T) -> Request<T> {
        let mut req = Request::new(inner);
        req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).unwrap(),
        );
        req
    }

    fn mock_developer_session(user_id: api_db::UserId) -> MockStore {
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
            .returning(|_| Ok(vec![api_db::Role::Developer]));
        store
    }

    fn mock_maintainer_session(user_id: api_db::UserId) -> MockStore {
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
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![api_db::Role::ProjectMaintainer]));
        store
    }

    fn mock_authed_session(user_id: api_db::UserId) -> MockStore {
        let uid = user_id.clone();
        let mut store = MockStore::new();
        store.expect_lookup_session().returning(move |_| {
            Ok(Some(api_db::SessionRow {
                user_id: uid.clone(),
                expires_at: api_db::chrono::Utc::now() + std::time::Duration::from_secs(3600),
                restricted: true,
            }))
        });
        store
    }

    fn sample_project_row() -> api_db::ProjectRow {
        api_db::ProjectRow {
            id: api_db::ProjectId::new("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap(),
            name: api_db::ProjectName::new("my-project").unwrap(),
            description: "A test project".to_owned(),
            visibility: api_db::ProjectVisibility::Public,
            embargoed_by_default: false,
            created_at: api_db::chrono::Utc::now(),
        }
    }

    // ── CreateProject ────────────────────────────────────────────────

    #[tokio::test]
    async fn create_project_succeeds_for_developer() {
        let user_id = api_db::UserId::new();
        let mut store = mock_developer_session(user_id);
        store
            .expect_create_project()
            .returning(|_, _, _, _, _| Ok(sample_project_row()));

        let handler = ProjectHandler::new(Arc::new(store));
        let resp = handler
            .create_project(authed_request(
                &"a".repeat(64),
                CreateProjectRequest {
                    name: "my-project".to_owned(),
                    description: "A test project".to_owned(),
                    visibility: causes_proto::project::Visibility::Public.into(),
                    embargoed_by_default: false,
                },
            ))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.project.unwrap().name, "my-project");
    }

    #[tokio::test]
    async fn create_project_rejects_non_developer() {
        let user_id = api_db::UserId::new();
        let mut store = mock_authed_session(user_id);
        store
            .expect_get_user_instance_roles()
            .returning(|_| Ok(vec![]));

        let handler = ProjectHandler::new(Arc::new(store));
        let err = handler
            .create_project(authed_request(
                &"a".repeat(64),
                CreateProjectRequest {
                    name: "my-project".to_owned(),
                    ..Default::default()
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    #[tokio::test]
    async fn create_project_rejects_invalid_name() {
        let user_id = api_db::UserId::new();
        let store = mock_developer_session(user_id);

        let handler = ProjectHandler::new(Arc::new(store));
        let err = handler
            .create_project(authed_request(
                &"a".repeat(64),
                CreateProjectRequest {
                    name: "INVALID".to_owned(),
                    ..Default::default()
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    // ── GetProject ───────────────────────────────────────────────────

    #[tokio::test]
    async fn get_project_returns_project() {
        let user_id = api_db::UserId::new();
        let mut store = mock_authed_session(user_id);
        store
            .expect_get_project()
            .returning(|_, _| Ok(api_db::ProjectAccess::Visible(sample_project_row())));

        let handler = ProjectHandler::new(Arc::new(store));
        let resp = handler
            .get_project(authed_request(
                &"a".repeat(64),
                GetProjectRequest {
                    project_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.project.unwrap().name, "my-project");
    }

    #[tokio::test]
    async fn get_project_returns_not_found() {
        let user_id = api_db::UserId::new();
        let mut store = mock_authed_session(user_id);
        store
            .expect_get_project()
            .returning(|_, _| Ok(api_db::ProjectAccess::NotFound));

        let handler = ProjectHandler::new(Arc::new(store));
        let err = handler
            .get_project(authed_request(
                &"a".repeat(64),
                GetProjectRequest {
                    project_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    // ── ListProjects ─────────────────────────────────────────────────

    #[tokio::test]
    async fn list_projects_returns_all() {
        use tokio_stream::StreamExt;

        let user_id = api_db::UserId::new();
        let mut store = mock_authed_session(user_id);
        store
            .expect_list_projects()
            .returning(|_| Ok(vec![sample_project_row()]));

        let handler = ProjectHandler::new(Arc::new(store));
        let stream = handler
            .list_projects(authed_request(&"a".repeat(64), ListProjectsRequest {}))
            .await
            .unwrap()
            .into_inner();
        let batches: Vec<_> = stream.collect().await;
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].as_ref().unwrap().projects.len(), 1);
    }

    // ── RenameProject ────────────────────────────────────────────────

    #[tokio::test]
    async fn rename_project_succeeds_for_maintainer() {
        let user_id = api_db::UserId::new();
        let mut store = mock_maintainer_session(user_id);
        store
            .expect_rename_project()
            .returning(|_, _| Ok(Some(sample_project_row())));

        let handler = ProjectHandler::new(Arc::new(store));
        let resp = handler
            .rename_project(authed_request(
                &"a".repeat(64),
                RenameProjectRequest {
                    project_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
                    new_name: "renamed".to_owned(),
                },
            ))
            .await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn rename_project_rejects_non_maintainer() {
        let user_id = api_db::UserId::new();
        let mut store = mock_authed_session(user_id);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![]));

        let handler = ProjectHandler::new(Arc::new(store));
        let err = handler
            .rename_project(authed_request(
                &"a".repeat(64),
                RenameProjectRequest {
                    project_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
                    new_name: "renamed".to_owned(),
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }

    // ── DeleteProject ────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_project_succeeds_for_maintainer() {
        let user_id = api_db::UserId::new();
        let mut store = mock_maintainer_session(user_id);
        store.expect_delete_project().returning(|_| Ok(true));

        let handler = ProjectHandler::new(Arc::new(store));
        let resp = handler
            .delete_project(authed_request(
                &"a".repeat(64),
                DeleteProjectRequest {
                    project_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
                },
            ))
            .await;
        assert!(resp.is_ok());
    }

    #[tokio::test]
    async fn delete_project_rejects_non_maintainer() {
        let user_id = api_db::UserId::new();
        let mut store = mock_authed_session(user_id);
        store
            .expect_get_user_project_roles()
            .returning(|_, _| Ok(vec![]));

        let handler = ProjectHandler::new(Arc::new(store));
        let err = handler
            .delete_project(authed_request(
                &"a".repeat(64),
                DeleteProjectRequest {
                    project_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_owned(),
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::PermissionDenied);
    }
}
