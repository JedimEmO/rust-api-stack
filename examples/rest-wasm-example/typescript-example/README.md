# REST TypeScript client sample

The calls a browser frontend makes to the [REST backend](../rest-backend/README.md)
through a TypeScript client generated from its OpenAPI document.
This is not an npm project; [src/example.ts](src/example.ts) only shows the call shapes.

## Getting the OpenAPI document

Build the backend from the repository root:

```bash
cargo check -p rest-backend --locked
```

The build script writes the document to
`examples/rest-wasm-example/rest-backend/target/openapi/userservice.json`.
A running backend also serves it at `http://127.0.0.1:3000/api/v1/docs/openapi.json`.

## What the sample assumes

`example.ts` imports a client from `src/generated`, which git ignores.
Generate it from the document with your OpenAPI client generator.
The sample expects one function per operation, named after the operation id,
such as `getUsers`, `getUsersId` and `postUsers`,
each returning `{ data, error }`.
Rename the imports if your generator names things differently.

The document's paths are relative to the service, so the sample passes `http://localhost:3000/api/v1` as `baseUrl`.
It shows public calls, bearer-token headers for the protected endpoints,
typed request bodies and snake_case path parameters such as `user_id`.
