use std::sync::Arc;

use tonic::Status;

use crate::store::Store;

/// Extract the bearer token from request metadata, validate the session,
/// and return the authenticated `UserId`.
///
/// Called explicitly by authenticated RPC handlers. Unauthenticated RPCs
/// (StartLogin, CompleteLogin, health) simply don't call this.
pub async fn authenticate<S: Store>(
    store: &Arc<S>,
    metadata: &tonic::metadata::MetadataMap,
) -> Result<api_db::UserId, Status> {
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

    Ok(session.user_id)
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
            Ok(Some(api_db::SessionRow::new_for_test(
                api_db::UserId::new(),
                true,
            )))
        });
        let store = Arc::new(store);

        let md = metadata_with_bearer(&"b".repeat(64));
        let err = authenticate(&store, &md).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
        assert!(err.message().contains("expired"));
    }

    #[tokio::test]
    async fn valid_token_returns_user_id() {
        let user_id = api_db::UserId::new();
        let uid = user_id.clone();
        let mut store = MockStore::new();
        store
            .expect_lookup_session()
            .returning(move |_| Ok(Some(api_db::SessionRow::new_for_test(uid.clone(), false))));
        let store = Arc::new(store);

        let md = metadata_with_bearer(&"c".repeat(64));
        let result = authenticate(&store, &md).await.unwrap();
        assert_eq!(result, user_id);
    }
}
