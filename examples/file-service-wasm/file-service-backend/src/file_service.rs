use async_trait::async_trait;
use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::Response;
use file_service_api::{DocumentServiceFileError, DocumentServiceTrait, UploadResponse};
use ras_auth_core::AuthenticatedUser;
use std::sync::Arc;
use tracing::{debug, error};

use crate::storage::FileStorage;

#[derive(Clone)]
pub struct FileServiceImpl {
    storage: Arc<FileStorage>,
}

impl FileServiceImpl {
    pub fn new(storage: Arc<FileStorage>) -> Self {
        Self { storage }
    }

    async fn handle_multipart_upload(
        &self,
        mut multipart: axum::extract::Multipart,
    ) -> Result<UploadResponse, DocumentServiceFileError> {
        debug!("Starting multipart upload processing");

        while let Some(field) = multipart.next_field().await.map_err(|e| {
            error!("Failed to get next multipart field: {}", e);
            DocumentServiceFileError::UploadFailed(format!("Error parsing multipart: {}", e))
        })? {
            debug!("Processing field: {:?}", field.name());
            if field.name() == Some("file") {
                let file_name = field.file_name().unwrap_or("unknown").to_string();

                let content_type = field.content_type().map(|ct| ct.to_string());

                debug!("Receiving file: {} (type: {:?})", file_name, content_type);

                // Read file data
                let data = field.bytes().await.map_err(|e| {
                    error!("Failed to read field bytes: {:?}", e);
                    error!("Error type: {}", std::any::type_name_of_val(&e));
                    DocumentServiceFileError::UploadFailed(format!(
                        "Failed to read file data: {}",
                        e
                    ))
                })?;
                let data_vec = data.to_vec();

                // Save to storage
                let metadata = self
                    .storage
                    .save_file(data_vec, &file_name, content_type)
                    .await
                    .map_err(|e| {
                        error!("Failed to save file: {}", e);
                        DocumentServiceFileError::Internal(e.to_string())
                    })?;

                debug!(
                    file_id = %metadata.id,
                    stored_path = %metadata.stored_path.display(),
                    "Saved uploaded file"
                );

                return Ok(UploadResponse {
                    file_id: metadata.id,
                    file_name: metadata.original_name,
                    size: metadata.size,
                });
            }
        }

        Err(DocumentServiceFileError::UploadFailed(
            "No file field found in multipart data".to_string(),
        ))
    }
}

#[async_trait]
impl DocumentServiceTrait for FileServiceImpl {
    async fn upload(
        &self,
        multipart: axum::extract::Multipart,
    ) -> Result<UploadResponse, DocumentServiceFileError> {
        debug!("Handling public file upload");
        self.handle_multipart_upload(multipart).await
    }

    async fn upload_profile_picture(
        &self,
        user: &AuthenticatedUser,
        multipart: axum::extract::Multipart,
    ) -> Result<UploadResponse, DocumentServiceFileError> {
        debug!("Handling secure file upload for user: {}", user.user_id);

        self.handle_multipart_upload(multipart).await
    }

    async fn download(&self, file_id: String) -> Result<Response<Body>, DocumentServiceFileError> {
        debug!("Handling public file download: {}", file_id);

        let (data, metadata) = self.storage.get_file(&file_id).await.map_err(|e| {
            error!("Failed to get file: {}", e);
            match e.to_string().contains("not found") {
                true => DocumentServiceFileError::NotFound,
                false => DocumentServiceFileError::DownloadFailed(e.to_string()),
            }
        })?;

        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, data.len());

        // Set content type if available
        if let Some(meta) = metadata {
            if let Some(content_type) = meta.content_type {
                response = response.header(header::CONTENT_TYPE, content_type);
            }

            // Set content disposition for download
            response = response.header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", meta.original_name),
            );
        }

        response
            .body(Body::from(data))
            .map_err(|_| DocumentServiceFileError::Internal("Failed to build response".to_string()))
    }

    async fn download_secure(
        &self,
        user: &AuthenticatedUser,
        file_id: String,
    ) -> Result<Response<Body>, DocumentServiceFileError> {
        debug!(
            "Handling secure file download for user {}: {}",
            user.user_id, file_id
        );

        // In a real app, you might check if the user has access to this file
        self.download(file_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "testuser".to_string(),
            permissions: HashSet::from(["user".to_string()]),
            metadata: None,
        }
    }

    fn test_service(temp_dir: &TempDir) -> FileServiceImpl {
        FileServiceImpl::new(Arc::new(FileStorage::new(temp_dir.path())))
    }

    #[tokio::test]
    async fn download_returns_saved_file_with_headers() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = Arc::new(FileStorage::new(temp_dir.path()));
        let saved = storage
            .save_file(
                b"download body".to_vec(),
                "report.txt",
                Some("text/plain".to_string()),
            )
            .await
            .expect("save file");
        let service = FileServiceImpl::new(storage);

        let response = service.download(saved.id).await.expect("download response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "13");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"file.txt\""
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(&body[..], b"download body");
    }

    #[tokio::test]
    async fn download_missing_file_maps_to_not_found() {
        let temp_dir = TempDir::new().expect("temp dir");
        let service = test_service(&temp_dir);

        let error = service
            .download("missing".to_string())
            .await
            .expect_err("missing file should be not found");

        assert!(matches!(error, DocumentServiceFileError::NotFound));
    }

    #[tokio::test]
    async fn secure_download_uses_same_download_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let storage = Arc::new(FileStorage::new(temp_dir.path()));
        let saved = storage
            .save_file(b"secure body".to_vec(), "secure.bin", None)
            .await
            .expect("save file");
        let service = FileServiceImpl::new(storage);

        let response = service
            .download_secure(&test_user(), saved.id)
            .await
            .expect("secure download response");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");

        assert_eq!(&body[..], b"secure body");
    }
}
