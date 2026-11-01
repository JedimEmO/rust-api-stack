# File service with a generated TypeScript client

A `file_service!` contract shared by an Axum backend, the generated Rust client,
and a TypeScript sample that calls a client generated from the OpenAPI document.

- [`file-service-api/`](file-service-api/README.md) declares the upload and download endpoints.
- [`file-service-backend/`](file-service-backend/README.md) implements them and stores files on disk.
- [`typescript-example/`](typescript-example/README.md) shows the calls a browser frontend makes.

## Run

```bash
cargo run -p file-service-backend --locked
```

The backend listens on `127.0.0.1:3000` and serves the endpoints under `/api/documents`.
Building it also writes the OpenAPI document to
`examples/file-service-wasm/file-service-backend/target/openapi/documentservice.json`.

## The two clients

Both clients come from the same declaration.
The Rust client, with the `fs` feature, streams an upload from a file on disk:

```rust
let form = DocumentServiceUploadMultipart::new()
    .file("report.pdf", Some("report.pdf"), Some("application/pdf"))
    .await?;

let response = client.upload(form).await?;
```

The TypeScript sample expects a client generated from the OpenAPI document into `typescript-example/src/generated`,
which git ignores.
Uploads there take a browser `Blob` or `File`; see
[example.ts](typescript-example/src/example.ts).
