use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum_test::{
    TestServer,
    multipart::{MultipartForm, Part},
};
use ras_file_core::{
    DownloadResponse, FileError, FileRequestContext, IncomingFile, JsonResponse, bytes::Bytes,
};
use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod support;
use support::{MockAuthProvider, axum_transport, mock_http_server, mock_http_server_arc};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct UploadMetadata {
    title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
struct UploadResponse {
    file_id: String,
    size: u64,
    title: String,
    comment: Option<String>,
}

file_service!({
    service_name: Demo,
    base_path: "/files",
    openapi: true,
    endpoints: [
        UPLOAD WITH_PERMISSIONS(["user"]) upload multipart {
            max_total_bytes: 2048,
            reject_unknown_fields: true,
            parts: [
                file file {
                    required: true,
                    max_count: 1,
                    max_bytes: 1024,
                    content_types: ["application/octet-stream"],
                    filename: required,
                },
                json metadata: UploadMetadata {
                    required: true,
                    max_bytes: 256,
                    content_types: ["application/json"],
                },
                text comment {
                    required: false,
                    max_bytes: 128,
                },
            ],
        } -> UploadResponse,
        DOWNLOAD UNAUTHORIZED download/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: true,
        },
        DOWNLOAD OPTIONAL_AUTH peek/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: false,
        },
    ]
});

#[derive(Default)]
struct UploadState {
    bytes: Vec<u8>,
    metadata: Option<UploadMetadata>,
    comment: Option<String>,
}

type Storage = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Clone)]
struct DemoImpl {
    storage: Storage,
    consume_file: bool,
    aborts: Arc<Mutex<usize>>,
    begins: Arc<Mutex<usize>>,
}

impl DemoImpl {
    fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(Vec::new())),
            consume_file: true,
            aborts: Arc::new(Mutex::new(0)),
            begins: Arc::new(Mutex::new(0)),
        }
    }

    fn without_file_consumption(mut self) -> Self {
        self.consume_file = false;
        self
    }
}

#[async_trait::async_trait]
impl DemoTrait for DemoImpl {
    type UploadState = UploadState;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DemoUploadPath,
    ) -> ras_file_core::FileResult<Self::UploadState> {
        *self.begins.lock().unwrap() += 1;
        Ok(UploadState::default())
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DemoUploadPath,
        state: &mut Self::UploadState,
        part: &mut DemoUploadPart<'_>,
    ) -> ras_file_core::FileResult<()> {
        match part {
            DemoUploadPart::File(file) => {
                if self.consume_file {
                    read_all(file, &mut state.bytes).await?;
                }
            }
            DemoUploadPart::Metadata(metadata) => {
                state.metadata = Some(metadata.clone());
            }
            DemoUploadPart::Comment(comment) => {
                state.comment = Some(comment.clone());
            }
        }
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DemoUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> ras_file_core::FileResult<JsonResponse<UploadResponse>> {
        let metadata = state
            .metadata
            .ok_or_else(|| FileError::bad_request("metadata missing"))?;
        let id = format!("file-{}", self.storage.lock().unwrap().len());
        let size = state.bytes.len() as u64;
        self.storage.lock().unwrap().push((id.clone(), state.bytes));

        Ok(JsonResponse::created(UploadResponse {
            file_id: id,
            size,
            title: metadata.title,
            comment: state.comment,
        }))
    }

    async fn upload_abort(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DemoUploadPath,
        _state: Self::UploadState,
        _error: &FileError,
    ) {
        *self.aborts.lock().unwrap() += 1;
    }

    async fn download_by_file_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        path: DemoDownloadByFileIdPath,
    ) -> ras_file_core::FileResult<DownloadResponse> {
        let bytes = self
            .storage
            .lock()
            .unwrap()
            .iter()
            .find_map(|(id, bytes)| (id == &path.file_id).then(|| bytes.clone()))
            .ok_or(FileError::NotFound)?;

        DownloadResponse::bytes(bytes)
            .content_type("application/octet-stream")?
            .attachment(format!("{}.bin", path.file_id))
    }

    async fn peek_by_file_id(
        &self,
        ctx: &FileRequestContext<'_>,
        path: DemoPeekByFileIdPath,
    ) -> ras_file_core::FileResult<DownloadResponse> {
        // OPTIONAL_AUTH: the caller is surfaced through the context, never rejected.
        let caller = match ctx.user {
            Some(user) => user.user_id.clone(),
            None => "anonymous".to_string(),
        };
        DownloadResponse::bytes(Bytes::from(format!("{caller}:{}", path.file_id)))
            .content_type("application/octet-stream")?
            .attachment(format!("{}.txt", path.file_id))
    }
}

async fn read_all(file: &mut IncomingFile<'_>, out: &mut Vec<u8>) -> ras_file_core::FileResult<()> {
    while let Some(chunk) = file.next_chunk().await? {
        out.extend_from_slice(&chunk);
    }
    Ok(())
}

fn form(payload: impl Into<Vec<u8>>) -> MultipartForm {
    MultipartForm::new()
        .add_part(
            "file",
            Part::bytes(payload.into())
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        )
        .add_text("comment", "hello")
}

fn demo_server(service: DemoImpl) -> TestServer {
    mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    )
}

fn demo_server_arc(service: DemoImpl) -> Arc<TestServer> {
    mock_http_server_arc(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    )
}

fn demo_client(server: Arc<TestServer>) -> DemoClient {
    DemoClient::builder("http://test.local")
        .build_with_transport(axum_transport(server))
        .expect("build DemoClient over AxumTestTransport")
}

#[path = "e2e/schema_client.rs"]
mod schema_client;
#[path = "e2e/transfers.rs"]
mod transfers;
#[path = "e2e/upload_limits.rs"]
mod upload_limits;
#[path = "e2e/upload_validation.rs"]
mod upload_validation;
