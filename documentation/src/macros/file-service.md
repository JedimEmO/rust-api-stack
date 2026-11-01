# `file_service!`

Use `file_service!` when the API handles uploads or downloads.

File traffic has different constraints from JSON requests.
The server should authenticate before it reads the body,
reject oversized requests early, validate each multipart field,
and stream bytes instead of buffering whole files.
That is why file endpoints get their own macro, separate from `rest_service!`.

From one definition the macro generates an Axum router, a handler trait,
a typed Rust client with multipart builders, and an optional OpenAPI document.

## Dependencies and features

Put the file service definition in a shared API crate. If you want generated
transport code to stay optional, expose API-crate features that forward to the
macro crate features:

```toml
[dependencies]
ras-file-macro = { version = "0.3", default-features = false }
ras-file-core = { version = "0.2.1", optional = true }
ras-auth-core = { version = "0.3", optional = true }
serde = { version = "1.0", features = ["derive"] }
async-trait = { version = "0.1", optional = true }
ras-transport-core = { version = "0.2", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
axum = { version = "0.8", optional = true }
schemars = { version = "1.2", optional = true }
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

The generated server refers to `ras_file_core`, `ras_auth_core`, `axum`,
`async_trait` and `serde_json` by path, so the crate that invokes the macro
must depend on them. `schemars` is needed only with `openapi` enabled.
The generated client refers to `ras_transport_core`.

Keep the feature named `fs` in your API crate.
The generated helper that streams a file part from disk is behind
`#[cfg(feature = "fs")]`, and that `cfg` is checked in *your* crate,
not in `ras-file-macro`.

The `ras-file-macro` features are:

| Feature | Default | What it does |
| --- | --- | --- |
| `server` | yes | Emits the handler trait, the router builder, and the OpenAPI functions. |
| `client` | no | Emits the client, its builder with `build_with_transport(...)`, and the multipart builders. |
| `reqwest` | yes | Implies `client`, and adds a `build()` that uses `ras_transport_core::ReqwestTransport`. |
| `fs` | no | Implies `client`, and enables `ras-transport-core/fs` for streaming file parts from disk. |
| `permissions` | no | Emits a permission manifest; see [Permission manifests](#permission-manifests). |

If a crate only injects a custom transport, forward
`ras-file-macro/client` plus `dep:ras-transport-core` instead of
`ras-file-macro/reqwest`.

Cargo unifies features across a workspace, so a `server` feature enabled by one
crate turns the server on for every crate that uses the macro.
To prevent that, set `feature_gated: true` in the definition.
The macro then emits both sides behind the invoking crate's own
`server`, `client` and `reqwest` features, whatever features `ras-file-macro` has.
That crate must declare features with exactly those names.

## Define the service

```rust,ignore
use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UploadResponse {
    pub file_id: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UploadMetadata {
    pub title: String,
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

The endpoint grammar is:

```text
UPLOAD   AUTH path/{param: Type}/segment multipart { UPLOAD_OPTIONS } -> ResponseType
DOWNLOAD AUTH path/{param: Type}/segment { DOWNLOAD_OPTIONS }
```

Path segments are Rust identifiers, so use `profile_picture`, not `profile-picture`.
An upload is always a `POST` and a download is always a `GET`.
A download's options block is optional,
and a download declares no response type, because the handler returns `DownloadResponse`.

### Service options

| Option | Default | Meaning |
| --- | --- | --- |
| `service_name: Ident` | required | Prefix for every generated type. |
| `base_path: "..."` | required | Path the router nests all endpoints under. |
| `openapi: <bool>` or `openapi: { output: "..." }` | off | Emit the OpenAPI functions; `output` sets the file path. |
| `feature_gated: <bool>` | `false` | Gate generated code on the invoking crate's `server`, `client` and `reqwest` features. |
| `endpoints: [...]` | empty | The `UPLOAD` and `DOWNLOAD` endpoints. |

### Upload options

| Option | Default | Meaning |
| --- | --- | --- |
| `max_total_bytes: <int>` or `unlimited` | required | Cap on the whole request body. |
| `reject_unknown_fields: <bool>` | `true` | Reject a field that matches no declared part. With `false`, unknown fields are read and discarded, and still count toward `max_total_bytes`. |
| `parts: [...]` | required | At least one part. |

Each part starts with its kind and its field name.
The field name is the multipart field name on the wire.

```text
file name { PART_OPTIONS }
json name: Type { PART_OPTIONS }
text name { PART_OPTIONS }
```

A `file` part reaches the handler as a stream.
A `json` part is read into memory and deserialized into `Type`.
A `text` part is read into memory as a UTF-8 `String`.

| Part option | Default | Meaning |
| --- | --- | --- |
| `max_bytes: <int>` | required | Cap on this part. |
| `required: <bool>` | `false` | Reject the request when the part is missing. |
| `max_count: <int>` | `1` | How many times the field may appear. Must be greater than zero. |
| `content_types: ["..."]` | any | Allowed `Content-Type` values for the part. The match is exact, so `text/plain; charset=utf-8` does not match `text/plain`. |
| `filename: optional \| required \| forbidden` | `optional` | Filename policy. Valid only on `file` parts. |

### Download options

| Option | Default | Meaning |
| --- | --- | --- |
| `content_types: ["..."]` | empty | Content types listed in the OpenAPI document. |
| `ranges: <bool>` | `false` | Advertise `206 Partial Content` in the OpenAPI document. |

Both download options only describe the endpoint.
The generated server does not filter responses by content type and does not parse `Range`;
a handler that advertises ranges reads `ctx.range()` and builds the partial response itself.

## Generated names

For `service_name: DocumentService`, the macro generates:

| Item | Name |
| --- | --- |
| Handler trait | `DocumentServiceTrait` |
| Router builder | `DocumentServiceBuilder` |
| Error type alias | `DocumentServiceFileError` (= `ras_file_core::FileError`) |
| Client and its builder | `DocumentServiceClient`, `DocumentServiceClientBuilder` |
| Path struct per endpoint | `DocumentServiceUploadPath`, `DocumentServiceDownloadByFileIdPath` |
| Part enum per upload | `DocumentServiceUploadPart` |
| Multipart builder per upload | `DocumentServiceUploadMultipart` |

Method names come from the path.
Segments are joined with `_`, and a path parameter becomes `by_<name>`,
so `download/{file_id: String}` generates `download_by_file_id`.
Each part becomes an enum variant in PascalCase, so the `metadata` part is
`DocumentServiceUploadPart::Metadata`.

## Implement the upload lifecycle

The generated server authenticates and checks permissions
before it reads the body.
It then calls the handler in phases:
`<endpoint>_begin` once, `<endpoint>_part` for each accepted part,
and `<endpoint>_finish` after the last part.
Each upload endpoint also adds an associated type `<Endpoint>State`,
which carries your state from one phase to the next.

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

The handler must read a file part to the end before it returns.
If it does not, the generated server rejects the request with `400`.
Call `file.drain().await?` to skip a part you do not need.

The trait also has an `<endpoint>_abort` method with an empty default body.
The server calls it, with the state and the error,
when the upload fails after `_begin` and before `_finish`:
a part breaks a limit or a policy, a required part is missing,
or `_part` returns an error.
Override it when temporary files or external reservations need cleanup.
`_abort` is not called when `_finish` itself fails, because `_finish` owns the state by then.

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

`DownloadResponse::bytes(...)` serves a body that is already in memory.

## Serve the router

`DocumentServiceBuilder::build()` returns an `axum::Router`
with every endpoint nested under `base_path`:

```rust,ignore
let app: axum::Router = DocumentServiceBuilder::new(MyService::default())
    .auth_provider(my_auth_provider)
    .build();
```

A `WITH_PERMISSIONS` endpoint answers `500` when no auth provider is set.
The builder also takes `auth_cookie(...)`, `auth_transport(...)` and
`csrf_protection(...)` for cookie auth,
and `with_usage_tracker(...)` and `with_duration_tracker(...)` for metrics.

## Auth syntax

File services use the same auth syntax as the other service macros
(see [Auth In The API Contract](../auth-in-api-contract.md)):

```rust,ignore
UNAUTHORIZED
OPTIONAL_AUTH
WITH_PERMISSIONS(["files:write"])
WITH_PERMISSIONS(["files:write", "tenant:active"])
WITH_PERMISSIONS(["admin"] | ["files:write", "tenant:active"])
WITH_PERMISSIONS([])
```

Use `WITH_PERMISSIONS([])` for authenticated-only file operations.
Use `OPTIONAL_AUTH` for a public download or upload that should still recognise a
signed-in caller.
The route is never rejected for auth reasons,
and the caller arrives through `FileRequestContext`,
not as a separate `Caller` parameter:
`ctx.user` is `Some(user)` for a valid credential and `None` otherwise.

## Use the generated Rust client

The generated client sets the bearer token, builds the multipart body,
and calls the upload and download endpoints.

Enable it through the API crate dependency:

```toml
[dependencies]
document-api = { path = "../document-api", default-features = false, features = ["client", "fs"] }
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

The multipart builder has one method per part, named after the part.
For a file part, `file(path, file_name, content_type)` streams from disk;
it needs the `fs` feature and is not available on `wasm32`.

For tests, browsers, or content already in memory,
use the generated `*_bytes` method for the file part:

```rust,ignore
let form = DocumentServiceUploadMultipart::new()
    .file_bytes(
        b"hello".to_vec(),
        "hello.txt",
        Some("text/plain"),
    )?;

let uploaded = client.upload(form).await?;
```

Each endpoint method also has a `*_with_timeout` variant that overrides the default timeout.
A download returns a `ras_transport_core::TransportResponse`;
read it with `bytes()` or stream it with `into_body_stream()`.
A non-2xx answer becomes `TransportError::Status`.

## Use an OpenAPI TypeScript client

OpenAPI-generated browser clients usually model a multipart upload as an object
whose fields match the declared parts.
The operation IDs are `<operation>_<path>`,
such as `upload_upload` and `download_download_file_id`,
so a generator that names functions after the operation ID in camelCase
produces `uploadUpload` and `downloadDownloadFileId`:

```typescript
import { downloadDownloadFileId, uploadUpload } from './generated';

const baseUrl = 'http://localhost:3000/api/documents';
const headers = { Authorization: `Bearer ${token}` };

const uploaded = await uploadUpload({
  baseUrl,
  headers,
  body: { file },
});

const downloaded = await downloadDownloadFileId({
  baseUrl,
  headers,
  path: { file_id: uploaded.data.file_id },
});
```

## OpenAPI output

With `openapi: true`, the macro emits:

```rust,ignore
pub fn generate_documentservice_openapi() -> serde_json::Value;
pub fn generate_documentservice_openapi_to_file() -> std::io::Result<()>;
```

The `_to_file` function writes `target/openapi/documentservice.json`,
or the path given in `openapi: { output: "..." }`.

Upload operations include `multipart/form-data` schemas and an `x-ras-file`
extension for limits and part policies. Download operations document binary
responses, content types, and range support.

## Permission manifests

With the `permissions` feature, the macro also emits
`generate_documentservice_permission_manifest()` and a
`documentservice_permissions` module of permission constants.
The generated code refers to `ras_permission_manifest`, so add that crate too.
[Permission Manifests](../permission-manifests.md) shows how to use them.

## Examples

See
[examples/file-service-example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/file-service-example)
and
[examples/file-service-wasm](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/file-service-wasm).
