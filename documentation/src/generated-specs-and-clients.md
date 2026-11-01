# Generated Specs And Clients

The service macros use the Rust API definition to generate machine-readable
contracts and client helpers.

## OpenRPC

`jsonrpc_service!` can generate OpenRPC when the service enables
`openrpc: true` or `openrpc: { output: "path/to/file.json" }`.

```rust,ignore
pub fn generate_userservice_openrpc() -> serde_json::Value;
pub fn try_generate_userservice_openrpc()
    -> Result<serde_json::Value, ras_jsonrpc_types::SpecGenerationError>;
pub fn generate_userservice_openrpc_to_file() -> Result<(), std::io::Error>;
```

The document includes method names, request and response schemas, auth
extensions, flattened `x-permissions`, grouped `x-permission-groups`, version
metadata for versioned methods, authored examples and each method's `errors`.
It validates against the OpenRPC 1.3 meta-schema.

Parameters are described the way the server reads them: one descriptor per
field, called by name, when the request is an object with a fixed set of fields,
and otherwise a single raw `params` descriptor marked `x-ras-params-raw`. The
[`jsonrpc_service!` guide](macros/jsonrpc-service.md#openrpc-and-clients) has the
details, including examples and `x-ras-error-examples`.

## OpenAPI

`rest_service!` and `file_service!` can generate OpenAPI with `openapi: true`
or a custom output path.

```rust,ignore
pub fn generate_userservice_openapi() -> serde_json::Value;
pub fn generate_userservice_openapi_to_file() -> std::io::Result<()>;
// rest_service! only:
pub fn try_generate_userservice_openapi()
    -> Result<serde_json::Value, ras_rest_core::SpecGenerationError>;
```

REST operations include routes, HTTP methods, JSON schemas, bearer auth
requirements, flattened `x-permissions`, grouped `x-permission-groups` and
authored examples, keyed by name on parameters, request body and response; see
the [`rest_service!` guide](macros/rest-service.md#examples). REST documents
validate against the OpenAPI 3.0 schema.
File-service operations also include
multipart schemas, binary download responses, and `x-ras-file` metadata for
upload limits, part policies, content types, and range support.

## Schemas And Components

`jsonrpc_service!` and `rest_service!` generate schemas with schemars 1.2 or
later, in the dialect the format expects: JSON Schema draft 7 for OpenRPC and the
OpenAPI 3.0 Schema Object for OpenAPI. Named types become shared entries in
`components.schemas`. `file_service!` has its own generator and does not take
examples.

Request and parameter schemas describe what the server reads, and response
schemas describe what it sends. When a type's two forms differ, for instance
through `#[serde(rename(serialize = ..., deserialize = ...))]` or an `Option`
field that is always sent, the response form gets its own component,
`<Name>.Serialize`.

The `try_generate_*` functions report an example that cannot be serialized, or
that does not read back unchanged, as a `SpecGenerationError`. A build script or
a test that calls them catches a broken example before the document is served.

## Rust Clients

Enabling a service macro crate's `client` feature emits typed Rust clients that
can be constructed with `build_with_transport(...)`. Enabling the macro crate's
`reqwest` feature also emits the default reqwest-backed `build()`.

The examples keep API definitions in separate API crates and expose API-crate
`client` features that forward to the macro crate's `reqwest` feature, so
server and browser crates can depend on the same contract while selecting
different generated surfaces. Test-only or in-process clients can instead
forward only the macro crate's `client` feature and depend directly on
`ras-transport-core`.

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
