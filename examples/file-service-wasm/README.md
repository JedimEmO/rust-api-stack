# File Service OpenAPI Usage Sample

This example demonstrates the `file_service!` macro with an Axum backend,
generated OpenAPI, and a minimal TypeScript usage sample that calls a generated
fetch client.

## Structure

- `file-service-api/` - The API library crate that contains the service definition
- `file-service-backend/` - Axum server implementation and OpenAPI generation
- `typescript-example/` - Minimal TypeScript usage sample for a generated client

## How it Works

1. The `file_service!` macro defines upload, download, and secured file endpoints.
2. The backend build script writes the generated OpenAPI document.
3. A TypeScript OpenAPI generator can create a fetch client from the generated document.
4. `typescript-example/src/example.ts` shows the generated client call shape.

## Running the Example

Run these commands from the repository root.

```bash
cargo check -p file-service-backend --locked
```

To exercise the calls manually, run the backend at `http://localhost:3000` and
use the functions shown in `typescript-example/src/example.ts`.

## Native And Browser Client Shape

The Rust client and TypeScript usage sample both come from the same API
definition. Native Rust uploads stream files from disk:

```rust
let form = DocumentServiceUploadMultipart::new()
    .file("report.pdf", Some("report.pdf"), Some("application/pdf"))
    .await?;

let response = client.upload(form).await?;
```

The TypeScript sample assumes a generated fetch client at
`typescript-example/src/generated`, which is intentionally ignored. That client
uses browser `Blob | File` values for multipart uploads; see
`typescript-example/src/example.ts`.

## TypeScript Usage

See `typescript-example/src/example.ts` for public upload/download calls and
the bearer-token variants for protected endpoints.

## Server Implementation

The backend depends on the shared API crate with the `server` feature enabled
and implements the generated `DocumentServiceTrait`. The checked-in
implementation is in [file_service.rs](file-service-backend/src/file_service.rs)
and stores uploaded files through [storage.rs](file-service-backend/src/storage.rs).
