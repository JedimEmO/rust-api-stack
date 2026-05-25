# Generated Specs And Clients

The service macros use the Rust API definition to generate machine-readable
contracts and client helpers.

## OpenRPC

`jsonrpc_service!` can generate OpenRPC when the service enables
`openrpc: true` or `openrpc: { output: "path/to/file.json" }`.

```rust,ignore
pub fn generate_userservice_openrpc() -> serde_json::Value;
pub fn generate_userservice_openrpc_to_file() -> Result<(), std::io::Error>;
```

The document includes method names, request and response schemas, auth
extensions, permissions, and version metadata for versioned methods.

## OpenAPI

`rest_service!` and `file_service!` can generate OpenAPI with `openapi: true`
or a custom output path.

```rust,ignore
pub fn generate_userservice_openapi() -> serde_json::Value;
pub fn generate_userservice_openapi_to_file() -> std::io::Result<()>;
```

REST operations include routes, HTTP methods, JSON schemas, bearer auth
requirements, and permission metadata. File-service operations also include
multipart schemas, binary download responses, and `x-ras-file` metadata for
upload limits, part policies, content types, and range support.

## Rust Clients

The shared API crate's `client` feature generates typed Rust clients. The
examples keep API definitions in separate API crates so server and browser
crates can depend on the same contract while enabling different API-crate
features.

For browser targets, compile client crates with `--target wasm32-unknown-unknown`
and enable only the API crate's client-side feature set. See:

- [examples/wasm-ui-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/wasm-ui-demo)
- [examples/rest-wasm-example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/rest-wasm-example)
- [examples/file-service-wasm](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/file-service-wasm)

## Build-Time Spec Generation

Backend crates can emit OpenRPC or OpenAPI during compilation from `build.rs`.
That keeps generated client input tied to the same Rust API contract used by
the server.

```rust,ignore
fn main() {
    rest_api::generate_userservice_openapi_to_file()
        .expect("generate OpenAPI");
}
```

The REST and file-service examples write specs under `target/openapi`. A
frontend build can then point its OpenAPI generator at that file.

## TypeScript Call Shape

The generated TypeScript fetch clients used in the examples accept one config
object per call:

```typescript
await postUsers({
  baseUrl: 'http://localhost:3000/api/v1',
  headers: { Authorization: `Bearer ${token}` },
  path: { id: 'user-123' },
  query: { include_archived: false },
  body: { name: 'Alice' },
});
```

Only include the fields the operation needs. Public `GET` calls often need only
`baseUrl`; protected uploads usually include `headers` and `body`.

## Versioned Methods And Endpoints

JSON-RPC and REST macros support opt-in compatibility definitions. A canonical
operation can declare legacy wire names, legacy request/response types, and a
migration type. The generated server accepts both shapes while the service
implementation only handles the canonical Rust type.

Use versioning when a deployed client still depends on an old wire contract and
the server can safely migrate requests and responses at the API boundary.
