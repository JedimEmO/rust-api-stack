# File Service API

Shared file-service contract for the [file service WASM/OpenAPI example](../README.md). This crate uses `ras-file-macro` to generate upload/download server traits, clients, and OpenAPI output for the backend and generated TypeScript usage sample.

## Generated Service

The contract in [src/lib.rs](src/lib.rs) defines `DocumentService` at base path `/api/documents` with a 100 MB body limit:

- `POST /api/documents/upload`
- `POST /api/documents/upload_profile_picture`
- `GET /api/documents/download/{file_id}`
- `GET /api/documents/download_secure/{file_id}`

The backend implementation is documented in [../file-service-backend/README.md](../file-service-backend/README.md). The plain TypeScript generated-client usage sample is documented in [../typescript-example/README.md](../typescript-example/README.md).

## Features

- `server` - marker feature used by the backend package when depending on this shared API crate.
- `wasm-client` - re-exports the macro-generated WASM client on `wasm32`.

## Checks

```bash
cargo check -p file-service-api --locked
cargo check -p file-service-api --features server --locked
cargo test -p file-service-api --locked
cargo clippy -p file-service-api --all-targets --all-features --locked -- -D warnings
```
