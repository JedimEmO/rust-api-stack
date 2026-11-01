# ras-file-macro

A procedural macro, `file_service!`, for typed file upload and download APIs.

Uploads and downloads need more care than JSON endpoints.
The server should check auth before it reads the body,
cap every multipart part, and stream bytes instead of buffering whole files.
`file_service!` writes that code from one definition:
a handler trait, an Axum router, a typed Rust client with multipart builders,
and an optional OpenAPI document.

## Example

```rust
use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileMetadata {
    pub id: String,
    pub filename: String,
    pub size: usize,
}

file_service!({
    service_name: FileStorage,
    base_path: "/api/files",
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
            ],
        } -> FileMetadata,
        DOWNLOAD WITH_PERMISSIONS(["files:read"]) download/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: true,
        },
        // Public download that still recognises a signed-in caller via
        // `FileRequestContext` (ctx.user is Some(..) when authenticated).
        DOWNLOAD OPTIONAL_AUTH preview/{file_id: String} {
            content_types: ["application/octet-stream"],
            ranges: false,
        },
    ]
});
```

This generates `FileStorageTrait` with `upload_begin`, `upload_part`,
`upload_finish` and `upload_abort` for the upload,
and `download_by_file_id` and `preview_by_file_id` for the downloads.
`FileStorageBuilder` turns an implementation into an `axum::Router`,
and `FileStorageClient` calls the same endpoints from Rust.
Upload handlers receive typed multipart parts,
and download handlers return `ras_file_core::DownloadResponse`.

## Installation

```toml
[dependencies]
ras-file-macro = "0.3"
ras-file-core = "0.2.1"
ras-auth-core = "0.3"
ras-transport-core = { version = "0.2", features = ["reqwest"] }
async-trait = "0.1"
axum = "0.8"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"
```

A proc-macro crate cannot re-export runtime crates,
so the crate that invokes the macro depends on what the generated code names:
`ras-file-core`, `ras-auth-core`, `axum`, `async-trait` and `serde_json` for the server,
`ras-transport-core` for the client, and `schemars` when `openapi` is on.

## Features

| Feature | Default | What it does |
| --- | --- | --- |
| `server` | yes | Handler trait, router builder and OpenAPI functions. |
| `client` | no | Client, `build_with_transport(...)` and multipart builders. |
| `reqwest` | yes | Implies `client`; adds a `build()` that uses `ReqwestTransport`. |
| `fs` | no | Implies `client`; enables streaming file parts from disk. |
| `permissions` | no | Permission manifest; needs `ras-permission-manifest`. |

The disk-streaming helper is behind `#[cfg(feature = "fs")]` in the generated code,
so the crate that invokes the macro must also declare a feature named `fs`.

## Documentation

The [`file_service!` guide](../../../documentation/src/macros/file-service.md)
covers the full grammar, the upload lifecycle, the generated client,
OpenAPI output, and how to split server and client features in a shared API crate.

## Checks

```bash
cargo test -p ras-file-macro --locked
cargo clippy -p ras-file-macro --all-targets --all-features --locked -- -D warnings
```
