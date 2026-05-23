//! Criterion bench measuring 1 MiB upload + download through the file_service!
//! in-memory axum-test router path.

use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_test::multipart::{MultipartForm, Part};
use criterion::{Criterion, criterion_group, criterion_main};
use ras_auth_core::AuthenticatedUser;
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

#[path = "../tests/support/mod.rs"]
mod support;
use support::{MockAuthProvider, mock_http_server};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadResponse {
    file_id: String,
    size: u64,
}

file_service!({
    service_name: BenchSvc,
    base_path: "/files",
    endpoints: [
        DOWNLOAD UNAUTHORIZED download/{file_id: String}(),
        UPLOAD WITH_PERMISSIONS(["user"]) upload() -> UploadResponse,
    ]
});

type Storage = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Clone)]
struct BenchImpl {
    storage: Storage,
}

#[async_trait::async_trait]
impl BenchSvcTrait for BenchImpl {
    async fn download(&self, file_id: String) -> Result<impl IntoResponse, BenchSvcFileError> {
        let bytes = self
            .storage
            .lock()
            .unwrap()
            .iter()
            .find_map(|(id, data)| (id == &file_id).then(|| data.clone()))
            .ok_or(BenchSvcFileError::NotFound)?;
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(bytes))
            .unwrap())
    }

    async fn upload(
        &self,
        _user: &AuthenticatedUser,
        mut multipart: axum::extract::Multipart,
    ) -> Result<UploadResponse, BenchSvcFileError> {
        let field = multipart
            .next_field()
            .await
            .map_err(|e| BenchSvcFileError::UploadFailed(e.to_string()))?
            .ok_or_else(|| BenchSvcFileError::UploadFailed("no field".into()))?;
        let data = field
            .bytes()
            .await
            .map_err(|e| BenchSvcFileError::UploadFailed(e.to_string()))?;
        let id = format!("file-{}", self.storage.lock().unwrap().len());
        let size = data.len() as u64;
        self.storage
            .lock()
            .unwrap()
            .push((id.clone(), data.to_vec()));
        Ok(UploadResponse { file_id: id, size })
    }
}

fn build_router() -> (axum::Router, Storage) {
    let storage: Storage = Arc::new(Mutex::new(Vec::new()));
    let router = BenchSvcBuilder::<BenchImpl, MockAuthProvider>::new(BenchImpl {
        storage: storage.clone(),
    })
    .auth_provider(MockAuthProvider::default())
    .build();
    (router, storage)
}

fn bench_streaming(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let payload: Vec<u8> = (0u8..=255).cycle().take(1024 * 1024).collect();
    let (router, _storage) = build_router();
    let server = Arc::new(mock_http_server(router));

    c.bench_function("file_upload_download_1mib", |b| {
        b.to_async(&rt).iter(|| {
            let server = Arc::clone(&server);
            let payload = payload.clone();
            async move {
                let form = MultipartForm::new().add_part(
                    "file",
                    Part::bytes(payload)
                        .file_name("blob.bin")
                        .mime_type("application/octet-stream"),
                );
                let response = server
                    .post("/files/upload")
                    .authorization_bearer("user-token")
                    .multipart(form)
                    .await;
                response.assert_status_ok();
                let r: UploadResponse = response.json();

                let response = server.get(&format!("/files/download/{}", r.file_id)).await;
                response.assert_status_ok();
                let bytes = response.into_bytes();
                std::hint::black_box(bytes);
            }
        });
    });
}

criterion_group!(benches, bench_streaming);
criterion_main!(benches);
