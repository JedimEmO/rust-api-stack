use axum::body::Body;
use axum::response::{IntoResponse, Response};
use ras_file_macro::file_service;

file_service!({
    service_name: TestParen,
    base_path: "/api",
    endpoints: [
        DOWNLOAD UNAUTHORIZED download/{id: String}(),
    ]
});

// Implement the service
#[derive(Clone)]
struct TestService;

#[async_trait::async_trait]
impl TestParenTrait for TestService {
    async fn download(&self, id: String) -> Result<impl IntoResponse, TestParenFileError> {
        Ok(Response::builder()
            .header("content-type", "text/plain")
            .body(Body::from(format!("Download {}", id)))
            .unwrap())
    }
}

#[tokio::test]
async fn generated_trait_handles_download_endpoint_with_path_parameter() {
    let response = TestService
        .download("report.txt".to_string())
        .await
        .expect("download succeeds")
        .into_response();

    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("body bytes");

    assert_eq!(parts.status, axum::http::StatusCode::OK);
    assert_eq!(&body[..], b"Download report.txt");
}
