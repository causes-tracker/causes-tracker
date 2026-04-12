//! Extensions for working with tonic/gRPC in Causes.

/// Tonic interceptor that injects a Bearer token into every gRPC request.
#[derive(Clone)]
pub struct BearerInterceptor(String);

impl BearerInterceptor {
    pub fn new(token: String) -> Self {
        Self(token)
    }
}

impl tonic::service::Interceptor for BearerInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        let value = format!("Bearer {}", self.0)
            .parse()
            .map_err(|_| tonic::Status::internal("invalid token"))?;
        req.metadata_mut().insert("authorization", value);
        Ok(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_interceptor_sets_authorization() {
        let mut interceptor = BearerInterceptor::new("tok123".to_string());
        let req =
            tonic::service::Interceptor::call(&mut interceptor, tonic::Request::new(())).unwrap();
        let auth = req
            .metadata()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(auth, "Bearer tok123");
    }
}
