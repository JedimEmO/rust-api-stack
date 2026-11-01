//! A read error on an upload part ends the part, even when the handler swallows it.

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum_test::multipart::{MultipartForm, Part};
use ras_file_core::{
    FileError, FileRequestContext, IncomingFile, JsonResponse, bytes::Bytes, futures_util,
};
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};

mod support;
use support::{MockAuthProvider, mock_http_server};

#[tokio::test]
async fn exceeding_the_file_limit_leaves_a_terminal_error() {
    let stream = futures_util::stream::iter([Ok(Bytes::from_static(b"too-large"))]);
    let mut file = IncomingFile::new("file", None, None, Default::default(), 3, Box::pin(stream));
    assert!(matches!(
        file.next_chunk().await,
        Err(FileError::PayloadTooLarge)
    ));
    // A handler may attempt to drain after an error to clean up the multipart
    // parser. Doing so must not turn a rejected part into a successful one.
    assert!(matches!(
        file.drain().await,
        Err(FileError::PayloadTooLarge)
    ));
    assert!(!file.is_finished());
    assert!(matches!(file.error(), Some(FileError::PayloadTooLarge)));
}

#[tokio::test]
async fn a_stream_error_is_terminal() {
    let stream = futures_util::stream::iter([
        Err(FileError::bad_request("malformed multipart body")),
        Ok(Bytes::from_static(b"after")),
    ]);
    let mut file = IncomingFile::new("file", None, None, Default::default(), 64, Box::pin(stream));
    assert!(matches!(
        file.next_chunk().await,
        Err(FileError::BadRequest(_))
    ));
    assert!(matches!(
        file.next_chunk().await,
        Err(FileError::BadRequest(_))
    ));
    assert!(!file.is_finished());
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Stored {
    bytes: usize,
}

file_service!({
    service_name: Recovering,
    base_path: "/recovering",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 1024,
            parts: [
                file file {
                    required: true,
                    max_bytes: 8,
                    filename: optional,
                },
            ],
        } -> Stored,
    ]
});

/// A handler that swallows a read error and drains the part, as a handler
/// treating the part as optional might.
#[derive(Clone, Default)]
struct RecoveringImpl {
    aborted_with: Arc<Mutex<Option<StatusCode>>>,
}

#[async_trait::async_trait]
impl RecoveringTrait for RecoveringImpl {
    type UploadState = usize;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &RecoveringUploadPath,
    ) -> ras_file_core::FileResult<Self::UploadState> {
        Ok(0)
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &RecoveringUploadPath,
        state: &mut Self::UploadState,
        part: &mut RecoveringUploadPart<'_>,
    ) -> ras_file_core::FileResult<()> {
        let RecoveringUploadPart::File(file) = part;
        loop {
            match file.next_chunk().await {
                Ok(Some(chunk)) => *state += chunk.len(),
                Ok(None) => break,
                Err(_) => {
                    let _ = file.drain().await;
                    break;
                }
            }
        }
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &RecoveringUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> ras_file_core::FileResult<JsonResponse<Stored>> {
        Ok(JsonResponse::ok(Stored { bytes: state }))
    }

    async fn upload_abort(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &RecoveringUploadPath,
        _state: Self::UploadState,
        error: &FileError,
    ) {
        *self.aborted_with.lock().unwrap() = Some(error.status());
    }
}

#[tokio::test]
async fn an_oversized_part_is_rejected_when_the_handler_swallows_the_error() {
    let service = RecoveringImpl::default();
    let aborted_with = service.aborted_with.clone();
    let server = mock_http_server(
        RecoveringBuilder::<RecoveringImpl, MockAuthProvider>::new(service).build(),
    );

    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(vec![b'x'; 64])
            .file_name("big.bin")
            .mime_type("application/octet-stream"),
    );
    let response = server.post("/recovering/upload").multipart(form).await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        *aborted_with.lock().unwrap(),
        Some(StatusCode::PAYLOAD_TOO_LARGE)
    );
}
