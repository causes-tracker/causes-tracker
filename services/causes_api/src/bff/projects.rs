use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::{Deserialize, Serialize};

use causes_proto::project_service_client::ProjectServiceClient;
use causes_proto::{
    CreateProjectRequest, DeleteProjectRequest, GetProjectRequest, ListProjectsRequest,
    RenameProjectRequest,
};

use super::{AppState, SessionToken, authed_channel, grpc_error_response};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/_internal/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/_internal/projects/{name}",
            get(get_project).put(rename_project).delete(delete_project),
        )
}

#[derive(Serialize)]
struct ProjectResponse {
    id: String,
    name: String,
    description: String,
    visibility: String,
    embargoed_by_default: bool,
}

fn visibility_str(v: i32) -> &'static str {
    match v {
        1 => "public",
        2 => "private",
        _ => "unknown",
    }
}

fn to_project_response(p: causes_proto::Project) -> ProjectResponse {
    ProjectResponse {
        id: p.id,
        name: p.name,
        description: p.description,
        visibility: visibility_str(p.visibility).to_string(),
        embargoed_by_default: p.embargoed_by_default,
    }
}

async fn list_projects(
    State(state): State<AppState>,
    session: Option<SessionToken>,
) -> impl IntoResponse {
    let session = match session {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    };

    let channel = match authed_channel(&state, &session).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let mut client = ProjectServiceClient::new(channel);

    match client.list_projects(ListProjectsRequest {}).await {
        Ok(resp) => {
            let mut stream = resp.into_inner();
            let mut projects = Vec::new();
            while let Some(batch) = tokio_stream::StreamExt::next(&mut stream).await {
                match batch {
                    Ok(b) => {
                        for p in b.projects {
                            projects.push(to_project_response(p));
                        }
                    }
                    Err(e) => return grpc_error_response(e).into_response(),
                }
            }
            axum::Json(projects).into_response()
        }
        Err(e) => grpc_error_response(e).into_response(),
    }
}

async fn get_project(
    State(state): State<AppState>,
    session: Option<SessionToken>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let session = match session {
        Some(s) => s,
        None => return (axum::http::StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    };

    let channel = match authed_channel(&state, &session).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let mut client = ProjectServiceClient::new(channel);

    match client.get_project(GetProjectRequest { name }).await {
        Ok(resp) => {
            let inner = resp.into_inner();
            match inner.project {
                Some(p) => axum::Json(to_project_response(p)).into_response(),
                None => (axum::http::StatusCode::NOT_FOUND, "project not found").into_response(),
            }
        }
        Err(e) => grpc_error_response(e).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateProjectBody {
    name: String,
    description: String,
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    embargoed_by_default: bool,
}

async fn create_project(
    State(state): State<AppState>,
    session: Option<SessionToken>,
    axum::Json(body): axum::Json<CreateProjectBody>,
) -> impl IntoResponse {
    let session = match session {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    };

    let channel = match authed_channel(&state, &session).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let mut client = ProjectServiceClient::new(channel);

    let visibility = match body.visibility.as_str() {
        "private" => 2,
        _ => 1, // default to public
    };

    match client
        .create_project(CreateProjectRequest {
            name: body.name,
            description: body.description,
            visibility,
            embargoed_by_default: body.embargoed_by_default,
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            match inner.project {
                Some(p) => {
                    (StatusCode::CREATED, axum::Json(to_project_response(p))).into_response()
                }
                None => (StatusCode::INTERNAL_SERVER_ERROR, "no project returned").into_response(),
            }
        }
        Err(e) => grpc_error_response(e).into_response(),
    }
}

#[derive(Deserialize)]
struct RenameProjectBody {
    new_name: String,
}

async fn rename_project(
    State(state): State<AppState>,
    session: Option<SessionToken>,
    Path(name): Path<String>,
    axum::Json(body): axum::Json<RenameProjectBody>,
) -> impl IntoResponse {
    let session = match session {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    };

    let channel = match authed_channel(&state, &session).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let mut client = ProjectServiceClient::new(channel);

    match client
        .rename_project(RenameProjectRequest {
            name,
            new_name: body.new_name,
        })
        .await
    {
        Ok(resp) => {
            let inner = resp.into_inner();
            match inner.project {
                Some(p) => axum::Json(to_project_response(p)).into_response(),
                None => (StatusCode::INTERNAL_SERVER_ERROR, "no project returned").into_response(),
            }
        }
        Err(e) => grpc_error_response(e).into_response(),
    }
}

async fn delete_project(
    State(state): State<AppState>,
    session: Option<SessionToken>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let session = match session {
        Some(s) => s,
        None => return (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
    };

    let channel = match authed_channel(&state, &session).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, msg.to_string()).into_response(),
    };
    let mut client = ProjectServiceClient::new(channel);

    match client.delete_project(DeleteProjectRequest { name }).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => grpc_error_response(e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::super::test_support::{start_mock_grpc, test_router};

    #[tokio::test]
    async fn list_projects_requires_auth() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/_internal/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_projects_returns_projects() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/_internal/projects")
                    .header("cookie", format!("causes_session={}", "d".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let projects = json.as_array().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["name"], "test-project");
        assert_eq!(projects[0]["visibility"], "public");
    }

    #[tokio::test]
    async fn get_project_returns_project() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/_internal/projects/test-project")
                    .header("cookie", format!("causes_session={}", "d".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "test-project");
    }

    #[tokio::test]
    async fn create_project_returns_201() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/_internal/projects")
                    .header("cookie", format!("causes_session={}", "d".repeat(64)))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "new-project",
                            "description": "A new project"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "test-project"); // mock always returns test-project
    }

    #[tokio::test]
    async fn create_project_requires_auth() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/_internal/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "new-project",
                            "description": "desc"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rename_project_returns_updated() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/_internal/projects/test-project")
                    .header("cookie", format!("causes_session={}", "d".repeat(64)))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"new_name": "renamed-project"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["name"].is_string());
    }

    #[tokio::test]
    async fn delete_project_returns_204() {
        let grpc_url = start_mock_grpc().await;
        let app = test_router(&grpc_url);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/_internal/projects/test-project")
                    .header("cookie", format!("causes_session={}", "d".repeat(64)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
