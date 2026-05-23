use axum::extract::Multipart;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider};
use ras_file_macro::file_service;

// Define response type
#[derive(serde::Serialize, serde::Deserialize)]
struct TestResponse {
    id: String,
}

// Simplest possible test
file_service!({
    service_name: MinimalService,
    base_path: "/api",
    endpoints: [
        UPLOAD UNAUTHORIZED upload() -> TestResponse,
    ]
});

// Implement the service
#[derive(Clone)]
struct MyService;

#[async_trait::async_trait]
impl MinimalServiceTrait for MyService {
    async fn upload(&self, _multipart: Multipart) -> Result<TestResponse, MinimalServiceFileError> {
        Ok(TestResponse {
            id: "test".to_string(),
        })
    }
}

// Dummy auth provider for testing
#[derive(Clone)]
struct DummyAuth;

impl AuthProvider for DummyAuth {
    fn authenticate(&self, _token: String) -> AuthFuture<'_> {
        Box::pin(async move { Err(AuthError::InvalidToken) })
    }
}

#[test]
fn generated_builder_accepts_service_and_auth_provider() {
    let service = MyService;
    let auth = DummyAuth;
    let _builder = MinimalServiceBuilder::new(service).auth_provider(auth);
}
