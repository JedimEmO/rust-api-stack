//! Criterion bench measuring 1 MiB upload + download through the file_service!
//! in-memory axum-test router path.

use std::sync::{Arc, Mutex};

use axum_test::multipart::{MultipartForm, Part};
use criterion::{Criterion, criterion_group, criterion_main};
use ras_file_core::{DownloadResponse, FileRequestContext, JsonResponse};
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
        DOWNLOAD UNAUTHORIZED download/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: false,
        },
        UPLOAD WITH_PERMISSIONS(["user"]) upload multipart {
            max_total_bytes: 2097152,
            reject_unknown_fields: true,
            parts: [
                file file {
                    required: true,
                    max_count: 1,
                    max_bytes: 2097152,
                    content_types: ["application/octet-stream"],
                    filename: optional,
                },
            ],
        } -> UploadResponse,
    ]
});

type Storage = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Clone)]
struct BenchImpl {
    storage: Storage,
}

#[derive(Default)]
struct UploadState {
    response: Option<UploadResponse>,
}

#[async_trait::async_trait]
impl BenchSvcTrait for BenchImpl {
    type UploadState = UploadState;

    async fn download_by_file_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        path: BenchSvcDownloadByFileIdPath,
    ) -> Result<DownloadResponse, BenchSvcFileError> {
        let bytes = self
            .storage
            .lock()
            .unwrap()
            .iter()
            .find_map(|(id, data)| (id == &path.file_id).then(|| data.clone()))
            .ok_or(BenchSvcFileError::NotFound)?;
        Ok(DownloadResponse::bytes(bytes))
    }

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &BenchSvcUploadPath,
    ) -> Result<Self::UploadState, BenchSvcFileError> {
        Ok(UploadState::default())
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &BenchSvcUploadPath,
        state: &mut Self::UploadState,
        part: &mut BenchSvcUploadPart<'_>,
    ) -> Result<(), BenchSvcFileError> {
        let BenchSvcUploadPart::File(file) = part;
        let mut data = Vec::new();
        while let Some(chunk) = file.next_chunk().await? {
            data.extend_from_slice(&chunk);
        }
        let id = format!("file-{}", self.storage.lock().unwrap().len());
        let size = data.len() as u64;
        self.storage.lock().unwrap().push((id.clone(), data));
        state.response = Some(UploadResponse { file_id: id, size });
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &BenchSvcUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> Result<JsonResponse<UploadResponse>, BenchSvcFileError> {
        Ok(JsonResponse::ok(state.response.ok_or_else(|| {
            BenchSvcFileError::handler_contract("upload finished without a file")
        })?))
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
