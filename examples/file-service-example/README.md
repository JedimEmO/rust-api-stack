# File service example

A small Axum server built from one `file_service!` declaration.
It keeps the contract and the implementation in a single crate,
so it is the shortest place to read how upload and download endpoints are declared and implemented.

It shows:

- a public download endpoint
- a multipart upload endpoint that requires the `upload` permission
- a download endpoint that requires the `admin` permission
- a fixed-token auth provider for local testing
- the request and duration hooks on the generated builder

The handlers return made-up content and store nothing.
[file-service-wasm](../file-service-wasm/) has a backend that writes uploads to disk.

## Run

```bash
cargo run -p file-service-example --locked
```

The server listens on `0.0.0.0:3000` and serves its routes under `/api/files`.

## Try it

Public download:

```bash
curl http://localhost:3000/api/files/download/test123
```

Upload as a user:

```bash
printf 'hello from rust-api-stack\n' > /tmp/ras-upload.txt
curl -X POST \
  -H 'Authorization: Bearer user-token' \
  -F 'file=@/tmp/ras-upload.txt' \
  http://localhost:3000/api/files/upload
```

The upload takes one multipart part named `file`, up to 50 MiB, and rejects any other part.

File information, admin only:

```bash
curl -H 'Authorization: Bearer admin-token' \
  http://localhost:3000/api/files/info/test123
```

## Tokens

- `user-token` has the `upload` permission.
- `admin-token` has the `upload` and `admin` permissions.

Any other bearer token is rejected.

## Checks

```bash
cargo test -p file-service-example --locked
cargo check -p file-service-example --no-default-features --features server --locked
cargo clippy -p file-service-example --all-targets --all-features --locked -- -D warnings
```
