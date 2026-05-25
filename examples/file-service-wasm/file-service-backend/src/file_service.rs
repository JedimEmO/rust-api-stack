use async_trait::async_trait;
use file_service_api::{
    DocumentServiceDownloadByFileIdPath, DocumentServiceDownloadSecureByFileIdPath,
    DocumentServiceFileError, DocumentServiceTrait, DocumentServiceUploadPart,
    DocumentServiceUploadPath, DocumentServiceUploadProfilePicturePart,
    DocumentServiceUploadProfilePicturePath, UploadResponse,
};
use ras_file_core::{DownloadResponse, FileRequestContext, IncomingFile, JsonResponse};
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

    async fn handle_file_upload(
        &self,
        file: &mut IncomingFile<'_>,
    ) -> Result<UploadResponse, DocumentServiceFileError> {
        let file_name = file.file_name().unwrap_or("unknown").to_string();
        let content_type = file.content_type().map(ToString::to_string);

        debug!("Receiving file: {} (type: {:?})", file_name, content_type);

        let mut data = Vec::new();
        while let Some(chunk) = file.next_chunk().await? {
            data.extend_from_slice(&chunk);
        }

        let metadata = self
            .storage
            .save_file(data, &file_name, content_type)
            .await
            .map_err(|e| {
                error!("Failed to save file: {}", e);
                DocumentServiceFileError::Internal
            })?;

        debug!(
            file_id = %metadata.id,
            stored_path = %metadata.stored_path.display(),
            "Saved uploaded file"
        );

        Ok(UploadResponse {
            file_id: metadata.id,
            file_name: metadata.original_name,
            size: metadata.size,
        })
    }

    async fn download_response(
        &self,
        file_id: String,
    ) -> Result<DownloadResponse, DocumentServiceFileError> {
        let (data, metadata) = self.storage.get_file(&file_id).await.map_err(|e| {
            error!("Failed to get file: {}", e);
            match e.to_string().contains("not found") {
                true => DocumentServiceFileError::NotFound,
                false => DocumentServiceFileError::download_failed(e.to_string()),
            }
        })?;

        let size = data.len() as u64;
        let mut response = DownloadResponse::bytes(data).content_length(size)?;

        if let Some(meta) = metadata {
            if let Some(content_type) = meta.content_type {
                response = response.content_type(content_type)?;
            }

            response = response.attachment(meta.original_name)?;
        }

        Ok(response)
    }
}

#[derive(Default)]
pub struct UploadState {
    response: Option<UploadResponse>,
}

#[async_trait]
impl DocumentServiceTrait for FileServiceImpl {
    type UploadState = UploadState;
    type UploadProfilePictureState = UploadState;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadPath,
    ) -> Result<Self::UploadState, DocumentServiceFileError> {
        debug!("Handling public file upload");
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
                state.response = Some(self.handle_file_upload(file).await?);
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
        let response = state.response.ok_or_else(|| {
            DocumentServiceFileError::handler_contract("upload finished without a file")
        })?;
        Ok(JsonResponse::ok(response))
    }

    async fn upload_profile_picture_begin(
        &self,
        ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadProfilePicturePath,
    ) -> Result<Self::UploadProfilePictureState, DocumentServiceFileError> {
        if let Some(user) = ctx.user {
            debug!("Handling secure file upload for user: {}", user.user_id);
        }
        Ok(UploadState::default())
    }

    async fn upload_profile_picture_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadProfilePicturePath,
        state: &mut Self::UploadProfilePictureState,
        part: &mut DocumentServiceUploadProfilePicturePart<'_>,
    ) -> Result<(), DocumentServiceFileError> {
        match part {
            DocumentServiceUploadProfilePicturePart::File(file) => {
                state.response = Some(self.handle_file_upload(file).await?);
            }
        }
        Ok(())
    }

    async fn upload_profile_picture_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DocumentServiceUploadProfilePicturePath,
        state: Self::UploadProfilePictureState,
        _summary: ras_file_core::UploadSummary,
    ) -> Result<JsonResponse<UploadResponse>, DocumentServiceFileError> {
        let response = state.response.ok_or_else(|| {
            DocumentServiceFileError::handler_contract("profile upload finished without a file")
        })?;
        Ok(JsonResponse::ok(response))
    }

    async fn download_by_file_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        path: DocumentServiceDownloadByFileIdPath,
    ) -> Result<DownloadResponse, DocumentServiceFileError> {
        debug!("Handling public file download: {}", path.file_id);
        self.download_response(path.file_id).await
    }

    async fn download_secure_by_file_id(
        &self,
        ctx: &FileRequestContext<'_>,
        path: DocumentServiceDownloadSecureByFileIdPath,
    ) -> Result<DownloadResponse, DocumentServiceFileError> {
        if let Some(user) = ctx.user {
            debug!(
                "Handling secure file download for user {}: {}",
                user.user_id, path.file_id
            );
        }

        // In a real app, you might check if the user has access to this file
        self.download_response(path.file_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode, header};
    use ras_auth_core::AuthenticatedUser;
    use ras_file_core::DownloadBody;
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

    fn test_context<'a>(
        headers: &'a HeaderMap,
        user: Option<&'a ras_auth_core::AuthenticatedUser>,
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
        let headers = HeaderMap::new();
        let ctx = test_context(&headers, None);

        let response = service
            .download_by_file_id(
                &ctx,
                DocumentServiceDownloadByFileIdPath { file_id: saved.id },
            )
            .await
            .expect("download response");

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.headers[header::CONTENT_LENGTH], "13");
        assert_eq!(response.headers[header::CONTENT_TYPE], "text/plain");
        assert_eq!(
            response.headers[header::CONTENT_DISPOSITION],
            "attachment; filename=\"file.txt\""
        );

        assert_eq!(body_bytes(response), b"download body");
    }

    #[tokio::test]
    async fn download_missing_file_maps_to_not_found() {
        let temp_dir = TempDir::new().expect("temp dir");
        let service = test_service(&temp_dir);
        let headers = HeaderMap::new();
        let ctx = test_context(&headers, None);

        let result = service
            .download_by_file_id(
                &ctx,
                DocumentServiceDownloadByFileIdPath {
                    file_id: "missing".to_string(),
                },
            )
            .await;
        let error = match result {
            Ok(_) => panic!("missing file should be not found"),
            Err(error) => error,
        };

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
        let headers = HeaderMap::new();
        let user = test_user();
        let ctx = test_context(&headers, Some(&user));

        let response = service
            .download_secure_by_file_id(
                &ctx,
                DocumentServiceDownloadSecureByFileIdPath { file_id: saved.id },
            )
            .await
            .expect("secure download response");

        assert_eq!(body_bytes(response), b"secure body");
    }
}
