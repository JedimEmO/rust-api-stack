# File service TypeScript client sample

The calls a browser frontend makes to the [file service backend](../file-service-backend/README.md)
through a TypeScript client generated from its OpenAPI document.
This is not an npm project; [src/example.ts](src/example.ts) only shows the call shapes.

## Getting the OpenAPI document

Build the backend from the repository root:

```bash
cargo check -p file-service-backend --locked
```

The build script writes the document to
`examples/file-service-wasm/file-service-backend/target/openapi/documentservice.json`.

## What the sample assumes

`example.ts` imports a client from `src/generated`, which git ignores.
Generate it from the document with your OpenAPI client generator.
The sample expects one function per operation, named after the operation id,
such as `uploadUpload` and `downloadDownloadFileId`,
each returning `{ data, error }`.
Rename the imports if your generator names things differently.

The sample shows public uploads and downloads,
bearer-token headers for the protected endpoints,
multipart bodies that take a `Blob` or `File`,
and the snake_case path parameter `file_id`.
