use axum::extract::Multipart;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider};
use ras_file_macro::file_service;

file_service!({
    service_name: SimpleService,
    base_path: "/api",
    endpoints: [
        UPLOAD UNAUTHORIZED upload() -> (),
    ]
});

#[derive(Clone)]
struct SimpleServiceImpl;

#[async_trait::async_trait]
impl SimpleServiceTrait for SimpleServiceImpl {
    async fn upload(&self, _multipart: Multipart) -> Result<(), SimpleServiceFileError> {
        Ok(())
    }
}

#[derive(Clone)]
struct RejectingAuth;

impl AuthProvider for RejectingAuth {
    fn authenticate(&self, _token: String) -> AuthFuture<'_> {
        Box::pin(async move { Err(AuthError::InvalidToken) })
    }
}

#[test]
fn generated_builder_accepts_unauthenticated_upload_service() {
    fn assert_trait_impl<T: SimpleServiceTrait>() {}
    assert_trait_impl::<SimpleServiceImpl>();

    let _builder = SimpleServiceBuilder::new(SimpleServiceImpl).auth_provider(RejectingAuth);
}
