//! Regression tests for F1 (upload filenames are sanitized before reaching the
//! handler) and F2 (axum rejection bodies are never echoed to the client).

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum_test::{
    TestServer,
    multipart::{MultipartForm, Part},
};
use ras_file_core::{DownloadResponse, FileError, FileRequestContext, JsonResponse};
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};

mod support;
use support::mock_http_server;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
struct SeenName {
    file_name: Option<String>,
}

file_service!({
    service_name: Sanitize,
    base_path: "/san",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 4096,
            reject_unknown_fields: false,
            parts: [
                file file {
                    required: true,
                    max_count: 1,
                    max_bytes: 1024,
                    filename: optional,
                },
            ],
        } -> SeenName,
        DOWNLOAD UNAUTHORIZED item/{id: u32} {
            content_types: ["application/octet-stream"],
            ranges: false,
        },
    ]
});

#[derive(Clone, Default)]
struct SanitizeImpl {
    seen: Arc<Mutex<Option<Option<String>>>>,
}

#[async_trait::async_trait]
impl SanitizeTrait for SanitizeImpl {
    type UploadState = Option<String>;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SanitizeUploadPath,
    ) -> ras_file_core::FileResult<Self::UploadState> {
        Ok(None)
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SanitizeUploadPath,
        state: &mut Self::UploadState,
        part: &mut SanitizeUploadPart<'_>,
    ) -> ras_file_core::FileResult<()> {
        let SanitizeUploadPart::File(file) = part;
        *state = file.file_name().map(ToString::to_string);
        while file.next_chunk().await?.is_some() {}
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SanitizeUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> ras_file_core::FileResult<JsonResponse<SeenName>> {
        *self.seen.lock().unwrap() = Some(state.clone());
        Ok(JsonResponse::ok(SeenName { file_name: state }))
    }

    async fn upload_abort(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SanitizeUploadPath,
        _state: Self::UploadState,
        _error: &FileError,
    ) {
    }

    async fn item_by_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        path: SanitizeItemByIdPath,
    ) -> ras_file_core::FileResult<DownloadResponse> {
        DownloadResponse::bytes(path.id.to_string().into_bytes())
            .content_type("application/octet-stream")
    }
}

fn server() -> TestServer {
    mock_http_server(
        SanitizeBuilder::<SanitizeImpl, support::MockAuthProvider>::new(SanitizeImpl::default())
            .build(),
    )
}

async fn upload_with_name(server: &TestServer, name: &str) -> SeenName {
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(b"hello".to_vec())
            .file_name(name)
            .mime_type("application/octet-stream"),
    );
    let response = server.post("/san/upload").multipart(form).await;
    response.assert_status_ok();
    response.json()
}

#[tokio::test]
async fn f1_upload_filename_traversal_is_reduced_to_final_component() {
    let server = server();
    let seen = upload_with_name(&server, "../../etc/passwd").await;
    assert_eq!(seen.file_name.as_deref(), Some("passwd"));

    let seen = upload_with_name(&server, "..\\..\\windows\\system.ini").await;
    assert_eq!(seen.file_name.as_deref(), Some("system.ini"));
}

#[tokio::test]
async fn f1_upload_filename_dot_only_or_empty_becomes_upload() {
    let server = server();
    assert_eq!(
        upload_with_name(&server, "..").await.file_name.as_deref(),
        Some("upload")
    );
    assert_eq!(
        upload_with_name(&server, "dir/").await.file_name.as_deref(),
        Some("upload")
    );
}

#[tokio::test]
async fn f1_upload_filename_unicode_is_preserved_and_length_capped() {
    let server = server();
    let seen = upload_with_name(&server, "ファイル.txt").await;
    assert_eq!(seen.file_name.as_deref(), Some("ファイル.txt"));

    let long = "é".repeat(200);
    let seen = upload_with_name(&server, &long).await;
    let name = seen.file_name.expect("name present");
    assert!(name.len() <= ras_file_core::MAX_FILENAME_BYTES);
    assert!(name.chars().all(|c| c == 'é'));
}

#[tokio::test]
async fn f2_invalid_path_parameter_returns_generic_message() {
    let server = server();
    let response = server.get("/san/item/not-a-number").await;
    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.text();
    assert!(
        !body.contains("not-a-number"),
        "path value must not be echoed: {body}"
    );
    assert_eq!(
        response.json::<serde_json::Value>()["error"],
        "invalid path parameters"
    );
}

#[tokio::test]
async fn f2_invalid_multipart_boundary_returns_generic_message() {
    let server = server();
    // multipart/form-data without a boundary makes the Multipart extractor
    // reject; axum's default body would be "Invalid `boundary` for
    // `Content-Type: multipart/form-data` request".
    let response = server
        .post("/san/upload")
        .bytes(b"garbage".to_vec().into())
        .content_type("multipart/form-data")
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.text();
    assert!(!body.contains("boundary"), "axum detail leaked: {body}");
    assert_eq!(
        response.json::<serde_json::Value>()["error"],
        "invalid multipart request"
    );
}

#[tokio::test]
async fn f2_malformed_multipart_body_returns_generic_message() {
    let server = server();
    let response = server
        .post("/san/upload")
        .bytes(
            b"--xyz\r\nthis is not a valid part header\r\n"
                .to_vec()
                .into(),
        )
        .content_type("multipart/form-data; boundary=xyz")
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);
    let body = response.text();
    assert_eq!(
        response.json::<serde_json::Value>()["error"],
        "invalid multipart body",
        "{body}"
    );
}
