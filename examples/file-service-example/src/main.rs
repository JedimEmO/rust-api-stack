use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_file_core::{DownloadResponse, FileRequestContext, JsonResponse};
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
        DOWNLOAD UNAUTHORIZED download/{file_id: String} {
            content_types: ["text/plain"],
            ranges: false,
        },

        // Authenticated upload endpoint
        UPLOAD WITH_PERMISSIONS(["upload"]) upload multipart {
            max_total_bytes: 52428800,
            reject_unknown_fields: true,
            parts: [
                file file {
                    required: true,
                    max_count: 1,
                    max_bytes: 52428800,
                    filename: optional,
                },
            ],
        } -> UploadResponse,

        // Admin-only file info endpoint
        DOWNLOAD WITH_PERMISSIONS(["admin"]) info/{file_id: String} {
            content_types: ["application/json"],
            ranges: false,
        },
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

#[derive(Default)]
struct UploadState {
    file_id: Option<String>,
    filename: Option<String>,
    size: u64,
}

#[async_trait::async_trait]
impl DocumentServiceTrait for DocumentServiceImpl {
    type UploadState = UploadState;

    async fn download_by_file_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        path: DocumentServiceDownloadByFileIdPath,
    ) -> Result<DownloadResponse, DocumentServiceFileError> {
        // In a real implementation, this would stream from storage
        let content = format!("File content for {}", path.file_id);

        DownloadResponse::bytes(content)
            .content_type("text/plain")?
            .attachment(format!("{}.txt", path.file_id))
    }

    async fn info_by_file_id(
        &self,
        ctx: &FileRequestContext<'_>,
        path: DocumentServiceInfoByFileIdPath,
    ) -> Result<DownloadResponse, DocumentServiceFileError> {
        let user = ctx.user.ok_or(DocumentServiceFileError::Unauthorized)?;
        println!(
            "Admin {} requesting info for file {}",
            user.user_id, path.file_id
        );

        // In a real implementation, this would fetch from database
        let info = FileInfo {
            id: path.file_id.clone(),
            name: format!("{}.pdf", path.file_id),
            size: 1024 * 1024, // 1MB
            content_type: "application/pdf".to_string(),
        };

        let body =
            serde_json::to_vec(&info).map_err(|_error| DocumentServiceFileError::Internal)?;
        DownloadResponse::bytes(body).content_type("application/json")
    }

    async fn upload_begin(
        &self,
        ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadPath,
    ) -> Result<Self::UploadState, DocumentServiceFileError> {
        if let Some(user) = ctx.user {
            println!("User {} is uploading a file", user.user_id);
        }

        Ok(UploadState::default())
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadPath,
        state: &mut Self::UploadState,
        part: &mut DocumentServiceUploadPart<'_>,
    ) -> Result<(), DocumentServiceFileError> {
        match part {
            DocumentServiceUploadPart::File(file) => {
                let file_name = file.file_name().unwrap_or("unknown").to_string();
                let field_name = file.field_name().to_string();
                let mut size = 0_u64;

                while let Some(chunk) = file.next_chunk().await? {
                    size += chunk.len() as u64;
                }

                println!(
                    "Received field '{}' with filename '{}', size: {} bytes",
                    field_name, file_name, size
                );

                // In a real implementation, you would save this to storage.
                state.file_id = Some(format!("file_{}", Uuid::new_v4()));
                state.size = size;
                state.filename = Some(file_name);
            }
        }

        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> Result<JsonResponse<UploadResponse>, DocumentServiceFileError> {
        Ok(JsonResponse::ok(UploadResponse {
            file_id: state.file_id.ok_or_else(|| {
                DocumentServiceFileError::handler_contract("upload finished without file id")
            })?,
            size: state.size,
            filename: state.filename.ok_or_else(|| {
                DocumentServiceFileError::handler_contract("upload finished without filename")
            })?,
        }))
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
        .with_usage_tracker(|_headers, method, path| {
            println!("Request: {} {}", method, path);
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
    use axum::http::{HeaderMap, StatusCode};
    use axum_test::{
        TestServer,
        multipart::{MultipartForm, Part},
    };
    use ras_file_core::DownloadBody;

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

    fn test_context<'a>(
        headers: &'a HeaderMap,
        user: Option<&'a AuthenticatedUser>,
    ) -> FileRequestContext<'a> {
        FileRequestContext::new("GET", "/test", "/test", headers, user)
    }

    fn body_bytes(response: DownloadResponse) -> Vec<u8> {
        match response.body {
            DownloadBody::Bytes(bytes) => bytes.to_vec(),
            DownloadBody::Empty | DownloadBody::Stream(_) => {
                panic!("expected in-memory response body")
            }
        }
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
        let headers = HeaderMap::new();
        let ctx = test_context(&headers, None);

        let response = service
            .download_by_file_id(
                &ctx,
                DocumentServiceDownloadByFileIdPath {
                    file_id: "test123".to_string(),
                },
            )
            .await
            .expect("download response");

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.headers["content-type"], "text/plain");
        assert_eq!(
            response.headers["content-disposition"],
            "attachment; filename=\"test123.txt\""
        );
        assert_eq!(body_bytes(response), b"File content for test123");
    }

    #[tokio::test]
    async fn info_returns_demo_metadata_for_admin_user() {
        let service = DocumentServiceImpl;
        let admin = test_user("admin-456", &["admin", "upload"]);
        let headers = HeaderMap::new();
        let ctx = test_context(&headers, Some(&admin));

        let response = service
            .info_by_file_id(
                &ctx,
                DocumentServiceInfoByFileIdPath {
                    file_id: "report".to_string(),
                },
            )
            .await
            .expect("info response");

        assert_eq!(response.status, StatusCode::OK);

        let body = body_bytes(response);
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
