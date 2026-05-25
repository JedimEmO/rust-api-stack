# File Service Example

Small Axum server demonstrating the `file_service!` macro for upload and download routes.

## What It Shows

- Public file download endpoint.
- Authenticated multipart upload endpoint.
- Admin-only file metadata endpoint.
- Simple bearer-token auth provider for local testing.
- Request and duration hooks emitted from the generated service builder.

## Run It

```bash
cargo run -p file-service-example --locked
```

The server listens on `http://localhost:3000` and serves routes under `/api/files`.

## Try It

Public download:

```bash
curl http://localhost:3000/api/files/download/test123
```

Authenticated upload:

```bash
printf 'hello from rust-api-stack\n' > /tmp/ras-upload.txt
curl -X POST \
  -H 'Authorization: Bearer user-token' \
  -F 'file=@/tmp/ras-upload.txt' \
  http://localhost:3000/api/files/upload
```

Admin file info:

```bash
curl -H 'Authorization: Bearer admin-token' \
  http://localhost:3000/api/files/info/test123
```

## Tokens

- `user-token`: has the `upload` permission.
- `admin-token`: has both `upload` and `admin` permissions.

Any other bearer token is rejected.

## Checks

```bash
cargo test -p file-service-example --locked
cargo check -p file-service-example --no-default-features --features server --locked
cargo clippy -p file-service-example --all-targets --all-features --locked -- -D warnings
```

## Notes

This example returns generated demo data instead of persisting files. Use `examples/file-service-wasm/file-service-backend` for a fuller backend with filesystem storage and CORS configuration.
