# File service API

The contract crate for the [file service example](../README.md).
It calls `file_service!` to declare `DocumentService`, which generates the server trait,
the router builder, the Rust client and the OpenAPI document.

## Endpoints

[src/lib.rs](src/lib.rs) mounts the service at `/api/documents`:

| Endpoint | Access | Notes |
|---|---|---|
| `POST /api/documents/upload` | public | one `file` part, up to 100 MiB |
| `POST /api/documents/upload_profile_picture` | `user` permission | one `file` part, PNG, JPEG or WebP, filename required |
| `GET /api/documents/download/{file_id}` | public | supports range requests |
| `GET /api/documents/download_secure/{file_id}` | `user` permission | supports range requests |

Both uploads reject unknown parts.
The backend is documented in [../file-service-backend/README.md](../file-service-backend/README.md)
and the TypeScript sample in [../typescript-example/README.md](../typescript-example/README.md).

## Features

- `server` generates the server trait, the router builder and the OpenAPI functions.
- `client` generates the Rust client with the default reqwest transport.
- `fs` adds client helpers that stream upload parts from files on disk.
- `wasm-client` enables `client` together with the browser helper dependencies.

## Checks

```bash
cargo check -p file-service-api --locked
cargo check -p file-service-api --features server --locked
cargo check -p file-service-api --features client --locked
cargo test -p file-service-api --locked
cargo test -p file-service-api --features server --locked
cargo clippy -p file-service-api --all-targets --all-features --locked -- -D warnings
```
