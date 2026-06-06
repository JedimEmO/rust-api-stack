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

#[tokio::test]
async fn upload_and_download_round_trips_declared_multipart_fields() {
    let service = DemoImpl::new();
    let storage = service.storage.clone();
    let server = demo_server_arc(service);
    let mut client = demo_client(server);
    client.set_bearer_token(Some("user-token"));

    let payload = b"streamed file".to_vec();
    let metadata = UploadMetadata {
        title: "demo".to_string(),
    };

    let form = DemoUploadMultipart::new()
        .file_bytes(
            payload.clone(),
            "blob.bin",
            Some("application/octet-stream"),
        )
        .expect("file part")
        .metadata(&metadata)
        .expect("json part")
        .comment("hello");

    let uploaded: UploadResponse = client.upload(form).await.expect("upload succeeds");
    assert_eq!(uploaded.size, payload.len() as u64);
    assert_eq!(uploaded.title, "demo");
    assert_eq!(uploaded.comment.as_deref(), Some("hello"));

    assert_eq!(storage.lock().unwrap().len(), 1);

    let response = client
        .download_by_file_id(uploaded.file_id.clone())
        .await
        .expect("download succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"file-0.bin\""
    );
    let downloaded = response.bytes().await.expect("download body");
    assert_eq!(downloaded.as_ref(), payload.as_slice());
}

#[cfg(all(not(target_arch = "wasm32"), feature = "fs"))]
#[tokio::test]
async fn upload_streams_file_part_from_disk_round_trips() {
    use std::io::Write as _;

    let service = DemoImpl::new();
    let storage = service.storage.clone();
    let server = demo_server_arc(service);
    let mut client = demo_client(server);
    client.set_bearer_token(Some("user-token"));

    // Write a temp file that the generated streaming `file(path, ...)` method
    // (tokio::fs::File -> ReaderStream -> MultipartBuilder::stream_part) reads
    // from disk. This is the only test that drives the from-disk streaming path.
    let payload = b"streamed-from-disk file contents".to_vec();
    let mut temp = tempfile::NamedTempFile::new().expect("create temp file");
    temp.write_all(&payload).expect("write temp file");
    temp.flush().expect("flush temp file");
    let path = temp.path().to_path_buf();

    let metadata = UploadMetadata {
        title: "demo".to_string(),
    };

    let form = DemoUploadMultipart::new()
        .file(&path, Some("blob.bin"), Some("application/octet-stream"))
        .await
        .expect("streaming file part from disk")
        .metadata(&metadata)
        .expect("json part")
        .comment("hello");

    let uploaded: UploadResponse = client.upload(form).await.expect("upload succeeds");
    assert_eq!(uploaded.size, payload.len() as u64);
    assert_eq!(uploaded.title, "demo");
    assert_eq!(uploaded.comment.as_deref(), Some("hello"));

    assert_eq!(storage.lock().unwrap().len(), 1);

    // Verify the exact bytes survived the from-disk streaming multipart framing.
    let response = client
        .download_by_file_id(uploaded.file_id.clone())
        .await
        .expect("download succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let downloaded = response.bytes().await.expect("download body");
    assert_eq!(downloaded.as_ref(), payload.as_slice());

    drop(temp);
}

#[tokio::test]
async fn generated_file_client_timeout_variants_round_trip() {
    let service = DemoImpl::new();
    let storage = service.storage.clone();
    let server = demo_server_arc(service);
    let mut client = demo_client(server);
    client.set_bearer_token(Some("user-token"));

    let payload = b"timeout upload".to_vec();
    let metadata = UploadMetadata {
        title: "timeout".to_string(),
    };
    let form = DemoUploadMultipart::new()
        .file_bytes(
            payload.clone(),
            "timeout.bin",
            Some("application/octet-stream"),
        )
        .expect("file part")
        .metadata(&metadata)
        .expect("json part");

    let uploaded = client
        .upload_with_timeout(form, std::time::Duration::from_secs(1))
        .await
        .expect("upload_with_timeout succeeds");
    assert_eq!(uploaded.size, payload.len() as u64);
    assert_eq!(storage.lock().unwrap().len(), 1);

    let response = client
        .download_by_file_id_with_timeout(uploaded.file_id, std::time::Duration::from_secs(1))
        .await
        .expect("download_by_file_id_with_timeout succeeds");
    let downloaded = response.bytes().await.expect("download body");
    assert_eq!(downloaded.as_ref(), payload.as_slice());
}

#[tokio::test]
async fn download_returns_not_found_for_missing_file() {
    let server = demo_server(DemoImpl::new());

    let response = server.get("/files/download/missing").await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[test]
fn generated_client_multipart_builder_covers_declared_parts() {
    let metadata = UploadMetadata {
        title: "demo".to_string(),
    };

    let (body, content_type) = DemoUploadMultipart::new()
        .file_bytes(
            b"body".to_vec(),
            "blob.bin",
            Some("application/octet-stream"),
        )
        .expect("file part")
        .metadata(&metadata)
        .expect("json part")
        .comment("hello")
        .into_body();

    assert!(content_type.starts_with("multipart/form-data; boundary="));
    match body {
        ras_transport_core::RequestBody::Stream(_) => {}
        other => panic!("expected streaming multipart body, got {other:?}"),
    }
}

#[tokio::test]
async fn upload_rejects_auth_before_beginning_upload() {
    let service = DemoImpl::new();
    let begins = service.begins.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let response = server.post("/files/upload").multipart(form("body")).await;

    response.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(*begins.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_request_content_type_before_beginning_upload() {
    let service = DemoImpl::new();
    let begins = service.begins.clone();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .text("not multipart")
        .content_type("text/plain")
        .await;

    response.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(*begins.lock().unwrap(), 0);
    assert_eq!(*aborts.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_content_length_over_total_before_beginning_upload() {
    let service = DemoImpl::new();
    let begins = service.begins.clone();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .add_header("content-length", "4096")
        .content_type("multipart/form-data; boundary=x")
        .bytes(Bytes::new())
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*begins.lock().unwrap(), 0);
    assert_eq!(*aborts.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_unsupported_file_content_type_after_begin_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.txt")
                .mime_type("text/plain"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_wrong_json_content_type_after_begin_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("text/plain"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_unknown_field_when_configured_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form("body").add_text("extra", "ignored?"))
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_duplicate_file_part_and_aborts_once() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = form("first").add_part(
        "file",
        Part::bytes("second")
            .file_name("second.bin")
            .mime_type("application/octet-stream"),
    );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_missing_required_filename_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body").mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_file_over_part_limit_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form(vec![b'x'; 1025]))
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_text_over_part_limit_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        )
        .add_text("comment", "x".repeat(129));

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_invalid_json_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text("{invalid").mime_type("application/json"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_invalid_utf8_text_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        )
        .add_part("comment", Part::bytes(vec![0xff, 0xfe]));

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_missing_required_field() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes("body")
            .file_name("blob.bin")
            .mime_type("application/octet-stream"),
    );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_when_handler_does_not_consume_file_stream() {
    let service = DemoImpl::new().without_file_consumption();
    let aborts = service.aborts.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form("body"))
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[test]
fn generated_openapi_documents_v2_multipart_contract() {
    let doc = generate_demo_openapi();

    let upload = &doc["paths"]["/upload"]["post"];
    assert_eq!(
        upload["requestBody"]["content"]["multipart/form-data"]["schema"]["properties"]["file"]["format"],
        "binary"
    );
    assert_eq!(upload["x-ras-file"]["maxTotalBytes"], 2048);
    assert_eq!(upload["x-permissions"], serde_json::json!(["user"]));

    let download = &doc["paths"]["/download/{file_id}"]["get"];
    assert_eq!(
        download["responses"]["200"]["content"]["application/octet-stream"]["schema"]["$ref"],
        "#/components/schemas/BinaryFileResponse"
    );
    assert_eq!(download["x-ras-file"]["ranges"], true);
}
