# `file_service!`

Use `file_service!` when the API handles uploads or downloads. It is separate
from the JSON REST macro because file traffic has different constraints:
authenticate before reading the body, reject oversized requests early, validate
multipart fields, and stream bytes instead of buffering entire files.

## Dependencies And Features

Put the file service definition in a shared API crate. If you want generated
transport code to stay optional, expose API-crate features that forward to the
macro crate features:

```toml
[dependencies]
ras-file-macro = { version = "0.1.0", default-features = false }
ras-file-core = { version = "0.1.0", optional = true }
ras-auth-core = { version = "0.1.0", optional = true }
serde = { version = "1.0", features = ["derive"] }
async-trait = { version = "0.1", optional = true }
ras-transport-core = { version = "0.1.0", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
axum = { version = "0.8", optional = true }
tokio = { version = "1.0", optional = true }
schemars = { version = "1.0.0-alpha.20", optional = true }
serde_json = { version = "1.0", optional = true }

[features]
default = []
server = [
    "ras-file-macro/server",
    "dep:ras-file-core",
    "dep:ras-auth-core",
    "dep:async-trait",
    "dep:axum",
    "dep:schemars",
    "dep:serde_json",
]
client = ["ras-file-macro/reqwest", "ras-transport-core/reqwest"]
fs = ["ras-file-macro/fs", "ras-transport-core/fs"]
```

Server crates depend on the API crate with `features = ["server"]`. Native and
browser clients depend on the same API crate with `features = ["client"]`.
Those API-crate features forward to the relevant macro crate features; the
macro emits only the selected generated surfaces.

Enable `fs` as well for native generated-client helpers that stream file parts
from disk.

The macro crate's `client` feature emits the generated client types and
`build_with_transport(...)`. Its `reqwest` feature also emits the default
reqwest-backed `build()`. If a crate only injects a custom transport, forward
`ras-file-macro/client` plus `dep:ras-transport-core` instead of
`ras-file-macro/reqwest`.

## Define The Service

```rust,ignore
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
            ],
        } -> UploadResponse,

        DOWNLOAD WITH_PERMISSIONS(["files:read"]) download/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: true,
        },
    ]
});
```

Every upload declares `max_total_bytes`, each part declares `max_bytes`, and
`reject_unknown_fields` defaults to `true`. File parts can require, forbid, or
allow filenames.

## Implement The Upload Lifecycle

Uploads are processed in phases. The generated server authenticates and checks
permissions before consuming the body, then calls service code as accepted parts
arrive.

```rust,ignore
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

If a file part is not fully consumed, the generated handler rejects the request.
Override the generated `*_abort` hook when temporary files or external
reservations need cleanup after an upload error.

## Downloads

Download handlers return `DownloadResponse`:

```rust,ignore
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

Path parameters become `by_*` method name segments. For example,
`download/{file_id: String}` generates `download_by_file_id`.

## Auth Syntax

File services use the same auth syntax as the other service macros —
`UNAUTHORIZED`, `OPTIONAL_AUTH`, and `WITH_PERMISSIONS([...])` (see
[Auth In The API Contract](../auth-in-api-contract.md)):

```rust,ignore
UNAUTHORIZED
OPTIONAL_AUTH
WITH_PERMISSIONS(["files:write"])
WITH_PERMISSIONS(["files:write", "tenant:active"])
WITH_PERMISSIONS(["admin"] | ["files:write", "tenant:active"])
WITH_PERMISSIONS([])
```

Use `WITH_PERMISSIONS([])` for authenticated-only file operations. Use
`OPTIONAL_AUTH` for a public download/upload that should still recognise a
signed-in caller: the route is never rejected for auth reasons, and the
(optional) caller is surfaced through `FileRequestContext` — `ctx.user` is
`Some(user)` for a valid credential and `None` otherwise — rather than as a
separate `Caller` parameter.

## Use The Generated Rust Client

The generated native client handles bearer auth, multipart construction, upload
methods, and download requests.

Enable it through the API crate dependency:

```toml
[dependencies]
document-api = { path = "../file-service-api", default-features = false, features = ["client", "fs"] }
```

```rust,ignore
let mut client = DocumentServiceClient::builder("http://localhost:3000")
    .with_timeout(std::time::Duration::from_secs(30))
    .build()?;

client.set_bearer_token(Some(user_token));

let metadata = UploadMetadata {
    title: "Quarterly report".to_string(),
};

let form = DocumentServiceUploadMultipart::new()
    .file("report.pdf", Some("report.pdf"), Some("application/pdf"))
    .await?
    .metadata(&metadata)?;

let uploaded = client.upload(form).await?;

let response = client.download_by_file_id(uploaded.file_id).await?;
let bytes = response.bytes().await?;
```

For tests, browser-like flows, or already-buffered content, use the generated
`*_bytes` helper for file parts:

```rust,ignore
let form = DocumentServiceUploadMultipart::new()
    .file_bytes(
        b"hello".to_vec(),
        "hello.txt",
        Some("text/plain"),
    )?;

let uploaded = client.upload(form).await?;
```

## Use An OpenAPI TypeScript Client

OpenAPI-generated browser clients usually model multipart uploads as an object
whose fields match the declared parts:

```typescript
import {
  downloadDownloadFileId,
  uploadUpload,
  uploadUploadProfilePicture,
} from './generated';

const baseUrl = 'http://localhost:3000/api/documents';

const uploaded = await uploadUpload({
  baseUrl,
  body: { file },
});

const secureUpload = await uploadUploadProfilePicture({
  baseUrl,
  headers: { Authorization: `Bearer ${token}` },
  body: { file },
});

const downloaded = await downloadDownloadFileId({
  baseUrl,
  path: { file_id: uploaded.data.file_id },
});
```

## OpenAPI And Clients

With `openapi: true`, the macro emits:

```rust,ignore
pub fn generate_documentservice_openapi() -> serde_json::Value;
pub fn generate_documentservice_openapi_to_file() -> std::io::Result<()>;
```

Upload operations include `multipart/form-data` schemas and an `x-ras-file`
extension for limits and part policies. Download operations document binary
responses, content types, and range support.

The native client feature generates multipart builders, including in-memory
`*_bytes` helpers for tests.

See
[examples/file-service-example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/file-service-example)
and
[examples/file-service-wasm](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/file-service-wasm).
