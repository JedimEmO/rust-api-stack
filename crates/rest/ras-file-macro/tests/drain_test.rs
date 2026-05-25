use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum_test::multipart::MultipartForm;
use ras_file_core::{FileError, FileRequestContext, JsonResponse, bytes::Bytes};
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};

mod support;
use support::{MockAuthProvider, mock_http_server};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DrainUploadResponse {
    title: String,
}

file_service!({
    service_name: DrainDemo,
    base_path: "/drain",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 2048,
            reject_unknown_fields: false,
            parts: [
                text title {
                    required: true,
                    max_bytes: 1024,
                },
            ],
        } -> DrainUploadResponse,
    ]
});

#[derive(Clone, Default)]
struct DrainImpl {
    aborts: Arc<Mutex<usize>>,
}

#[async_trait::async_trait]
impl DrainDemoTrait for DrainImpl {
    type UploadState = Option<String>;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DrainDemoUploadPath,
    ) -> ras_file_core::FileResult<Self::UploadState> {
        Ok(None)
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DrainDemoUploadPath,
        state: &mut Self::UploadState,
        part: &mut DrainDemoUploadPart<'_>,
    ) -> ras_file_core::FileResult<()> {
        match part {
            DrainDemoUploadPart::Title(title) => *state = Some(title.clone()),
            DrainDemoUploadPart::__Lifetime(_) => {}
        }
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DrainDemoUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> ras_file_core::FileResult<JsonResponse<DrainUploadResponse>> {
        Ok(JsonResponse::ok(DrainUploadResponse {
            title: state.ok_or_else(|| FileError::bad_request("title missing"))?,
        }))
    }

    async fn upload_abort(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &DrainDemoUploadPath,
        _state: Self::UploadState,
        _error: &FileError,
    ) {
        *self.aborts.lock().unwrap() += 1;
    }
}

fn server(service: DrainImpl) -> axum_test::TestServer {
    mock_http_server(DrainDemoBuilder::<DrainImpl, MockAuthProvider>::new(service).build())
}

fn raw_multipart(fields: &[(&str, Vec<u8>)]) -> (String, Bytes) {
    let boundary = "ras-file-test-boundary";
    let mut body = Vec::new();

    for (name, bytes) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (
        format!("multipart/form-data; boundary={boundary}"),
        body.into(),
    )
}

#[tokio::test]
async fn upload_drains_unknown_fields_when_reject_unknown_fields_is_false() {
    let service = DrainImpl::default();
    let aborts = service.aborts.clone();
    let server = server(service);

    let form = MultipartForm::new()
        .add_text("ignored", "1234567890")
        .add_text("title", "demo");

    let response = server.post("/drain/upload").multipart(form).await;

    response.assert_status_ok();
    let uploaded: DrainUploadResponse = response.json();
    assert_eq!(uploaded.title, "demo");
    assert_eq!(*aborts.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_when_drained_unknown_field_exceeds_total_limit() {
    let service = DrainImpl::default();
    let aborts = service.aborts.clone();
    let server = server(service);

    let (content_type, body) = raw_multipart(&[("ignored", vec![b'x'; 3000])]);

    let response = server
        .post("/drain/upload")
        .content_type(&content_type)
        .bytes(body)
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_when_known_part_pushes_drained_total_over_limit() {
    let service = DrainImpl::default();
    let aborts = service.aborts.clone();
    let server = server(service);

    let (content_type, body) =
        raw_multipart(&[("ignored", vec![b'x'; 1500]), ("title", vec![b'y'; 700])]);

    let response = server
        .post("/drain/upload")
        .content_type(&content_type)
        .bytes(body)
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}
