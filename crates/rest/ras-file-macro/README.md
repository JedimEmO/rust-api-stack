# ras-file-macro

Procedural macro for type-safe file upload and download services.

The `file_service!` macro generates the service trait, Axum routes, client
helpers, OpenAPI output, authentication checks, and file-specific error types
for a file API definition.

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
        UPLOAD WITH_PERMISSIONS(["files:write"]) upload() -> FileMetadata,
        DOWNLOAD WITH_PERMISSIONS(["files:read"]) download/{file_id: String}(),
    ]
});
```

See [documentation/ras-file-macro.md](../../../documentation/ras-file-macro.md)
for the usage guide and runnable examples.

## Checks

```bash
cargo test -p ras-file-macro --locked
cargo clippy -p ras-file-macro --all-targets --all-features --locked -- -D warnings
```
