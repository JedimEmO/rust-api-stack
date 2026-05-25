# File Service Macro

`ras-file-macro` generates focused file upload/download APIs from one service
definition. It is intentionally separate from the JSON REST macro because file
traffic has different constraints: authentication should happen before reading
the body, uploads need per-field and total byte limits, and handlers should be
able to stream bytes instead of receiving a fully buffered request.

The generated server adapts Axum multipart requests into runtime-neutral types
from `ras-file-core`:

- `FileRequestContext<'_>` carries method, matched path, headers, and the
  authenticated user.
- `IncomingFile<'_>` streams file chunks and enforces the declared part limit.
- `JsonResponse<T>` is returned by upload finish handlers.
- `DownloadResponse` is returned by download handlers.

## Definition Syntax

```rust
use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UploadResponse {
    pub file_id: String,
    pub size: u64,
}

file_service!({
    service_name: DocumentService,
    base_path: "/api/documents",
    openapi: true,
    endpoints: [
        UPLOAD WITH_PERMISSIONS(["files:write"]) upload multipart {
            max_total_bytes: 52428800,
            reject_unknown_fields: true,
            parts: [
                file file {
                    required: true,
                    max_count: 1,
                    max_bytes: 52428800,
                    content_types: ["application/pdf", "text/plain"],
                    filename: optional,
                },
                json metadata: UploadMetadata {
                    required: false,
                    max_bytes: 4096,
                    content_types: ["application/json"],
                },
                text comment {
                    required: false,
                    max_bytes: 1024,
                },
            ],
        } -> UploadResponse,

        DOWNLOAD WITH_PERMISSIONS(["files:read"]) download/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: true,
        },
    ]
});
```

`max_total_bytes` is required on every upload and may be `unlimited`. Every
part must declare `max_bytes`. `reject_unknown_fields` defaults to `true`.

File parts support `filename: optional`, `filename: required`, and
`filename: forbidden`. JSON parts require a Rust type after `:` and are decoded
before the service receives the part. Text parts are decoded as UTF-8.

## Generated Trait Shape

Uploads are a lifecycle. The service can allocate state after authentication
but before the body is consumed, handle each accepted part, then finish with a
JSON response.

```rust
use ras_file_core::{FileRequestContext, FileResult, JsonResponse};

#[async_trait::async_trait]
impl DocumentServiceTrait for MyService {
    type UploadState = UploadState;

    async fn upload_begin(
        &self,
        ctx: &FileRequestContext<'_>,
        path: &DocumentServiceUploadPath,
    ) -> FileResult<Self::UploadState> {
        Ok(UploadState::default())
    }

    async fn upload_part(
        &self,
        ctx: &FileRequestContext<'_>,
        path: &DocumentServiceUploadPath,
        state: &mut Self::UploadState,
        part: &mut DocumentServiceUploadPart<'_>,
    ) -> FileResult<()> {
        match part {
            DocumentServiceUploadPart::File(file) => {
                while let Some(chunk) = file.next_chunk().await? {
                    state.write(&chunk).await?;
                }
            }
            DocumentServiceUploadPart::Metadata(metadata) => {
                state.metadata = Some(metadata.clone());
            }
            DocumentServiceUploadPart::Comment(comment) => {
                state.comment = Some(comment.clone());
            }
        }

        Ok(())
    }

    async fn upload_finish(
        &self,
        ctx: &FileRequestContext<'_>,
        path: &DocumentServiceUploadPath,
        state: Self::UploadState,
        summary: ras_file_core::UploadSummary,
    ) -> FileResult<JsonResponse<UploadResponse>> {
        Ok(JsonResponse::ok(state.into_response()))
    }
}
```

If a file part is not fully consumed, the generated handler rejects the request
with a handler contract error. This keeps the multipart stream in a predictable
state and prevents accidental partial reads.

For every upload endpoint the macro also generates an optional `*_abort`
method. Override it when temporary files or external reservations need cleanup:

```rust
async fn upload_abort(
    &self,
    ctx: &FileRequestContext<'_>,
    path: &DocumentServiceUploadPath,
    state: Self::UploadState,
    error: &ras_file_core::FileError,
) {
    state.cleanup().await;
}
```

Downloads return a `DownloadResponse`:

```rust
use ras_file_core::{DownloadResponse, FileRequestContext, FileResult};

async fn download_by_file_id(
    &self,
    ctx: &FileRequestContext<'_>,
    path: DocumentServiceDownloadByFileIdPath,
) -> FileResult<DownloadResponse> {
    let file = self.storage.open(&path.file_id).await?;

    DownloadResponse::stream(file.stream)
        .content_type(file.content_type)?
        .content_length(file.size)?
        .attachment(file.original_name)
}
```

Path parameters become `by_*` method segments. For example,
`download/{file_id: String}` generates `download_by_file_id` and
`DocumentServiceDownloadByFileIdPath`.

## Early Rejection

The generated server performs these checks before calling service code:

- authentication, CSRF, and permission checks before reading the upload body;
- `Content-Length` rejection when it exceeds `max_total_bytes`;
- `multipart/form-data` validation for uploads;
- unknown field rejection when `reject_unknown_fields` is true;
- per-part count, content type, filename policy, and byte-limit checks;
- required field checks before `*_finish`.

This gives the service implementation a narrow job: accept already-declared
parts, stream bytes to storage, and return typed responses.

## Clients

The generated native client accepts a generated multipart builder:

```rust
let form = DocumentServiceUploadMultipart::new()
    .file("report.pdf", Some("report.pdf"), Some("application/pdf"))
    .await?
    .metadata(&metadata)?
    .comment("quarterly report");

let response = client.upload(form).await?;
```

Each file part also has a `*_bytes` helper for tests and in-memory uploads.

## OpenAPI

With `openapi: true`, the macro emits `generate_<service>_openapi()` and
`generate_<service>_openapi_to_file()`. Upload operations include an inline
`multipart/form-data` schema plus an `x-ras-file` extension describing
`maxTotalBytes`, unknown-field policy, and part limits. Download operations
document binary responses and an `x-ras-file` extension for declared content
types and range support.

## Checks

```bash
cargo test -p ras-file-macro --locked
cargo test -p file-service-example --locked
cargo test -p file-service-api -p file-service-backend --locked
```
