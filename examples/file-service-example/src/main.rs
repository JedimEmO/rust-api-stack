use axum::{
    body::Body,
    extract::Multipart,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct UploadResponse {
    file_id: String,
    size: u64,
    filename: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileInfo {
    id: String,
    name: String,
    size: u64,
    content_type: String,
}

// Define the file service
file_service!({
    service_name: DocumentService,
    base_path: "/api/files",
    endpoints: [
        // Public download endpoint
        DOWNLOAD UNAUTHORIZED download/{file_id: String}(),

        // Authenticated upload endpoint
        UPLOAD WITH_PERMISSIONS(["upload"]) upload() -> UploadResponse,

        // Admin-only file info endpoint
        DOWNLOAD WITH_PERMISSIONS(["admin"]) info/{file_id: String}() -> FileInfo,
    ]
});

// Simple auth provider for demo
#[derive(Clone)]
struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            match token.as_str() {
                "user-token" => Ok(AuthenticatedUser {
                    user_id: "user-123".to_string(),
                    permissions: vec!["upload".to_string()]
                        .into_iter()
                        .collect::<HashSet<_>>(),
                    metadata: None,
                }),
                "admin-token" => Ok(AuthenticatedUser {
                    user_id: "admin-456".to_string(),
                    permissions: vec!["upload".to_string(), "admin".to_string()]
                        .into_iter()
                        .collect::<HashSet<_>>(),
                    metadata: None,
                }),
                _ => Err(AuthError::InvalidToken),
            }
        })
    }
}

// Service implementation
#[derive(Clone)]
struct DocumentServiceImpl;

#[async_trait::async_trait]
impl DocumentServiceTrait for DocumentServiceImpl {
    async fn download(
        &self,
        file_id: String,
    ) -> Result<impl IntoResponse, DocumentServiceFileError> {
        // In a real implementation, this would stream from storage
        let content = format!("File content for {}", file_id);
        let body = Body::from(content);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .header(
                "content-disposition",
                format!("attachment; filename=\"{}.txt\"", file_id),
            )
            .body(body)
            .map_err(|e| DocumentServiceFileError::DownloadFailed(e.to_string()))?)
    }

    async fn upload(
        &self,
        user: &AuthenticatedUser,
        mut multipart: Multipart,
    ) -> Result<UploadResponse, DocumentServiceFileError> {
        println!("User {} is uploading a file", user.user_id);

        // Process the first multipart field — that's the uploaded file in the
        // demo's contract. Real implementations would loop and accept several.
        let field = multipart
            .next_field()
            .await
            .map_err(|e| {
                DocumentServiceFileError::UploadFailed(format!("Failed to get next field: {}", e))
            })?
            .ok_or_else(|| {
                DocumentServiceFileError::UploadFailed("No file in multipart data".to_string())
            })?;

        let name = field.name().unwrap_or("unknown").to_string();
        let file_name = field.file_name().unwrap_or("unknown").to_string();
        let data = field.bytes().await.map_err(|e| {
            DocumentServiceFileError::UploadFailed(format!("Failed to read field data: {}", e))
        })?;

        println!(
            "Received field '{}' with filename '{}', size: {} bytes",
            name,
            file_name,
            data.len()
        );

        // In a real implementation, you would save this to storage
        Ok(UploadResponse {
            file_id: format!("file_{}", Uuid::new_v4()),
            size: data.len() as u64,
            filename: file_name,
        })
    }

    async fn info(
        &self,
        user: &AuthenticatedUser,
        file_id: String,
    ) -> Result<impl IntoResponse, DocumentServiceFileError> {
        println!(
            "Admin {} requesting info for file {}",
            user.user_id, file_id
        );

        // In a real implementation, this would fetch from database
        let info = FileInfo {
            id: file_id.clone(),
            name: format!("{}.pdf", file_id),
            size: 1024 * 1024, // 1MB
            content_type: "application/pdf".to_string(),
        };

        Ok(axum::Json(info))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create service
    let service = DocumentServiceImpl;
    let auth = DemoAuthProvider;

    // Build the router
    let app = DocumentServiceBuilder::new(service)
        .auth_provider(auth)
        .with_usage_tracker(|headers, method, path| {
            println!("Request: {} {} - Headers: {:?}", method, path, headers);
        })
        .with_duration_tracker(|method, path, duration| {
            println!("Request {} {} took {:?}", method, path, duration);
        })
        .build()
        .layer(TraceLayer::new_for_http());

    println!("File service starting on http://0.0.0.0:3000");
    println!("Try:");
    println!("  curl http://localhost:3000/api/files/download/test123");
    println!(
        "  curl -X POST -H 'Authorization: Bearer user-token' -F 'file=@somefile.txt' http://localhost:3000/api/files/upload"
    );
    println!(
        "  curl -H 'Authorization: Bearer admin-token' http://localhost:3000/api/files/info/test123"
    );

    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, response::IntoResponse};
    use axum_test::{
        TestServer,
        multipart::{MultipartForm, Part},
    };

    fn test_user(user_id: &str, permissions: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_string(),
            permissions: permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect(),
            metadata: None,
        }
    }

    fn test_server() -> TestServer {
        let app = DocumentServiceBuilder::new(DocumentServiceImpl)
            .auth_provider(DemoAuthProvider)
            .build();

        TestServer::builder()
            .mock_transport()
            .build(app)
            .expect("in-memory axum-test server")
    }

    #[tokio::test]
    async fn demo_auth_provider_maps_user_and_admin_permissions() {
        let auth = DemoAuthProvider;

        let user = auth.authenticate("user-token".to_string()).await.unwrap();
        assert_eq!(user.user_id, "user-123");
        assert!(user.permissions.contains("upload"));
        assert!(!user.permissions.contains("admin"));

        let admin = auth.authenticate("admin-token".to_string()).await.unwrap();
        assert_eq!(admin.user_id, "admin-456");
        assert!(admin.permissions.contains("upload"));
        assert!(admin.permissions.contains("admin"));
    }

    #[tokio::test]
    async fn demo_auth_provider_rejects_unknown_tokens() {
        let auth = DemoAuthProvider;

        let error = auth
            .authenticate("not-a-token".to_string())
            .await
            .expect_err("unknown token should be rejected");

        assert!(matches!(error, AuthError::InvalidToken));
    }

    #[tokio::test]
    async fn download_returns_text_attachment() {
        let service = DocumentServiceImpl;

        let response = service
            .download("test123".to_string())
            .await
            .expect("download response")
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/plain");
        assert_eq!(
            response.headers()["content-disposition"],
            "attachment; filename=\"test123.txt\""
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        assert_eq!(&body[..], b"File content for test123");
    }

    #[tokio::test]
    async fn info_returns_demo_metadata_for_admin_user() {
        let service = DocumentServiceImpl;
        let admin = test_user("admin-456", &["admin", "upload"]);

        let response = service
            .info(&admin, "report".to_string())
            .await
            .expect("info response")
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let info: FileInfo = serde_json::from_slice(&body).expect("file info json");

        assert_eq!(info.id, "report");
        assert_eq!(info.name, "report.pdf");
        assert_eq!(info.size, 1024 * 1024);
        assert_eq!(info.content_type, "application/pdf");
    }

    #[tokio::test]
    async fn generated_public_download_route_works_without_token() {
        let server = test_server();

        let response = server.get("/api/files/download/test123").await;

        response.assert_status_ok();
        assert_eq!(response.headers()["content-type"], "text/plain");
        assert_eq!(
            response.headers()["content-disposition"],
            "attachment; filename=\"test123.txt\""
        );
        assert_eq!(response.text(), "File content for test123");
    }

    #[tokio::test]
    async fn generated_upload_route_accepts_user_token_and_multipart_file() {
        let server = test_server();
        let form = MultipartForm::new().add_part(
            "file",
            Part::bytes("example bytes")
                .file_name("example.txt")
                .mime_type("text/plain"),
        );

        let response = server
            .post("/api/files/upload")
            .authorization_bearer("user-token")
            .multipart(form)
            .await;

        response.assert_status_ok();
        let upload: UploadResponse = response.json();
        assert!(upload.file_id.starts_with("file_"));
        assert_eq!(upload.size, "example bytes".len() as u64);
        assert_eq!(upload.filename, "example.txt");
    }

    #[tokio::test]
    async fn generated_admin_info_route_enforces_admin_permission() {
        let server = test_server();

        let user_response = server
            .get("/api/files/info/report")
            .authorization_bearer("user-token")
            .await;
        user_response.assert_status(StatusCode::FORBIDDEN);

        let admin_response = server
            .get("/api/files/info/report")
            .authorization_bearer("admin-token")
            .await;
        admin_response.assert_status_ok();

        let info: FileInfo = admin_response.json();
        assert_eq!(info.id, "report");
        assert_eq!(info.name, "report.pdf");
    }
}
