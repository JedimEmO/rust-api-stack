# File service backend

The Axum server for the [file service example](../README.md).
It implements the `DocumentService` contract from [`file-service-api`](../file-service-api/README.md)
and stores uploaded files on disk.

## Run

From the workspace root:

```bash
cargo run -p file-service-backend --locked
```

The server listens on `127.0.0.1:3000`.
Uploads go to `./uploads` under the directory you start it from;
set `STORAGE_PATH` to use another directory.
The server also reads `STORAGE_PATH` from a `.env` file in the current directory;
[.env.example](.env.example) is a starting point.

Building the crate writes the OpenAPI document to `target/openapi/documentservice.json`
and the permission manifest to `target/ras-permissions/file-service-wasm.json`, both under this directory.

## Endpoints

- `POST /api/documents/upload`, public
- `GET /api/documents/download/{file_id}`, public
- `POST /api/documents/upload_profile_picture`, requires the `user` permission
- `GET /api/documents/download_secure/{file_id}`, requires the `user` permission

```bash
printf 'hello\n' > /tmp/ras-hello.txt
curl -F 'file=@/tmp/ras-hello.txt' http://127.0.0.1:3000/api/documents/upload
curl http://127.0.0.1:3000/api/documents/download/<file_id from the upload response>
```

## Authentication

A mock provider accepts one bearer token, `validtoken`, as user `testuser` with the `user` permission.
Any other token is rejected.

```bash
curl -H 'Authorization: Bearer validtoken' \
  -F 'file=@avatar.png;type=image/png' \
  http://127.0.0.1:3000/api/documents/upload_profile_picture
```

## Storage

Each upload gets a UUID as its file id.
Public uploads land in `public/` and authenticated uploads in `users/{user_id}/`,
so `download_secure` only finds files the same user uploaded:

```text
uploads/
├── public/{uuid}.{ext}
└── users/testuser/{uuid}.{ext}
```

The original extension is kept when it is one of a short list of safe types, such as `txt`, `png` or `pdf`;
anything else is stored as `.bin`.
Downloads guess the content type from the stored extension, keep it only when it is on a similar safe list,
fall back to `application/octet-stream`, and are sent as attachments,
so an uploaded file cannot be rendered as HTML in the browser.

CORS allows any origin, so a frontend on another port can call the backend.
Restrict it before you share the server.

## The implementation

[src/file_service.rs](src/file_service.rs) implements `DocumentServiceTrait`.
Each upload endpoint has three methods, called in order:
`upload_begin` creates per-request state, `upload_part` runs once per multipart part,
and `upload_finish` returns the JSON response.
Each download endpoint has one method that returns a `DownloadResponse`.
The generated router checks authentication, permissions and size limits before calling them.

## Checks

```bash
cargo test -p file-service-backend --locked
cargo clippy -p file-service-backend --all-targets --all-features --locked -- -D warnings
```
