//! End-to-end test for the file_service! macro: in-memory axum-test request
//! -> axum router -> handler. Exercises upload + download with byte-equality
//! and a missing-token rejection.

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_test::multipart::{MultipartForm, Part};
use ras_auth_core::AuthenticatedUser;
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};

mod support;
use support::{MockAuthProvider, mock_http_server};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadResponse {
    file_id: String,
    size: u64,
}

file_service!({
    service_name: Demo,
    base_path: "/files",
    endpoints: [
        DOWNLOAD UNAUTHORIZED download/{file_id: String}(),
        UPLOAD WITH_PERMISSIONS(["user"]) upload() -> UploadResponse,
    ]
});

type Storage = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Clone)]
struct DemoImpl {
    storage: Storage,
}

#[async_trait::async_trait]
impl DemoTrait for DemoImpl {
    async fn download(&self, file_id: String) -> Result<impl IntoResponse, DemoFileError> {
        let store = self.storage.lock().unwrap();
        let bytes = store
            .iter()
            .find_map(|(id, data)| (id == &file_id).then(|| data.clone()))
            .ok_or(DemoFileError::NotFound)?;

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(Body::from(bytes))
            .unwrap())
    }

    async fn upload(
        &self,
        _user: &AuthenticatedUser,
        mut multipart: axum::extract::Multipart,
    ) -> Result<UploadResponse, DemoFileError> {
        let field = multipart
            .next_field()
            .await
            .map_err(|e| DemoFileError::UploadFailed(e.to_string()))?
            .ok_or_else(|| DemoFileError::UploadFailed("no field".into()))?;
        let data = field
            .bytes()
            .await
            .map_err(|e| DemoFileError::UploadFailed(e.to_string()))?;
        let id = format!("file-{}", self.storage.lock().unwrap().len());
        let size = data.len() as u64;
        self.storage
            .lock()
            .unwrap()
            .push((id.clone(), data.to_vec()));
        Ok(UploadResponse { file_id: id, size })
    }
}

fn router(storage: Storage) -> axum::Router {
    DemoBuilder::<DemoImpl, MockAuthProvider>::new(DemoImpl { storage })
        .auth_provider(MockAuthProvider::default())
        .build()
}

#[tokio::test]
async fn upload_and_download_round_trips_bytes() {
    let storage: Storage = Arc::new(Mutex::new(Vec::new()));
    let server = mock_http_server(router(storage.clone()));

    let payload: Vec<u8> = (0u8..=255).cycle().take(64 * 1024).collect();
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(payload.clone())
            .file_name("blob.bin")
            .mime_type("application/octet-stream"),
    );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;
    response.assert_status_ok();
    let upload: UploadResponse = response.json();
    assert_eq!(upload.size, payload.len() as u64);

    let response = server
        .get(&format!("/files/download/{}", upload.file_id))
        .await;
    response.assert_status_ok();
    let bytes = response.into_bytes();
    assert_eq!(bytes.as_ref(), payload.as_slice());
}

#[tokio::test]
async fn upload_rejected_without_token() {
    let storage: Storage = Arc::new(Mutex::new(Vec::new()));
    let server = mock_http_server(router(storage));

    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes("hello world")
            .file_name("hi.txt")
            .mime_type("text/plain"),
    );

    let response = server.post("/files/upload").multipart(form).await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn download_unknown_file_returns_404() {
    let storage: Storage = Arc::new(Mutex::new(Vec::new()));
    let server = mock_http_server(router(storage));

    let response = server.get("/files/download/does-not-exist").await;
    response.assert_status(StatusCode::NOT_FOUND);
}
