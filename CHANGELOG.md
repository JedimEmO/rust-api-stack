# Changelog

Notable changes to the crates in this repository.
Each crate has its own version;
[VERSIONING.md](VERSIONING.md) says which changes bump which part.

## [Unreleased]

The first release on crates.io.
No earlier version was published,
so this section describes what the release contains rather than what changed.
The development history before it is in the git log.

### Crates

| Crate | Version |
| --- | --- |
| `ras-jsonrpc-macro` | 0.4.0 |
| `ras-rest-macro` | 0.4.0 |
| `ras-file-macro` | 0.3.0 |
| `ras-jsonrpc-bidirectional-macro` | 0.3.0 |
| `ras-jsonrpc-core` | 0.3.0 |
| `ras-jsonrpc-types` | 0.3.0 |
| `ras-rest-core` | 0.2.1 |
| `ras-file-core` | 0.2.1 |
| `ras-transport-core` | 0.2.0 |
| `ras-jsonrpc-bidirectional-server` | 0.4.0 |
| `ras-jsonrpc-bidirectional-client` | 0.4.0 |
| `ras-jsonrpc-bidirectional-types` | 0.3.0 |
| `ras-auth-core` | 0.3.0 |
| `ras-identity-core` | 0.1.1 |
| `ras-identity-local` | 0.3.0 |
| `ras-identity-oauth2` | 0.3.0 |
| `ras-identity-session` | 0.4.0 |
| `ras-observability-core` | 0.2.0 |
| `ras-observability-otel` | 0.2.0 |
| `ras-version-core` | 0.1.0 |
| `ras-openrpc-types` | 0.1.1 |
| `ras-permission-manifest` | 0.1.0 |
| `ras-spec-components` | 0.1.0 |
| `ras-api-explorer-assets` | 0.1.1 |

The versions are not all `0.1.0`, because they were bumped during development, before anything was published.
Publish order follows the dependencies: `cargo publish --workspace` handles it.

### Service macros

- `jsonrpc_service!` generates a JSON-RPC 2.0 service over HTTP on axum: a handler trait, a router builder and a typed Rust client.
  With `openrpc: true` it also builds an OpenRPC 1.3.2 document, with parameters by name, and can serve an API explorer page.
- `rest_service!` generates a JSON REST service with path and query parameters, a typed client, an OpenAPI 3.0.3 document and an explorer page.
- `file_service!` generates streaming multipart uploads and streaming downloads, a typed client and an OpenAPI document.
- `jsonrpc_bidirectional_service!` generates JSON-RPC in both directions over a WebSocket:
  client-to-server methods, server-to-client notifications and calls, and a native or WASM client.
- Methods and endpoints can carry typed request and response examples, including error examples,
  which appear in the OpenRPC and OpenAPI documents and in the explorer.
- Versioned JSON-RPC methods and REST endpoints migrate older request and response shapes with `ras-version-core`.
- `feature_gated: true` puts the generated server and client behind the API crate's own `server`, `client` and `reqwest` features,
  so a browser client does not compile axum.
- The generated code compiles in consumer crates on any edition from 2018.
- The crates need Rust 1.88 or newer.

### Auth

- All four macros share one auth syntax: `UNAUTHORIZED`, `OPTIONAL_AUTH`, and `WITH_PERMISSIONS([...])` with permission groups separated by `|`.
- `ras-auth-core` defines the `AuthProvider` trait and reads credentials from a bearer header or a cookie, with CSRF protection for cookies.
- `UNAUTHORIZED` and `OPTIONAL_AUTH` operations ignore a credential that fails to authenticate, in every macro.
- A JSON-RPC or WebSocket handler error reaches the client as a generic internal error,
  unless it is a `PublicRpcError`, whose code and message are meant for the client.
  A REST handler returns a `RestError`, which sets the status and message the client sees.
- With their `permissions` feature, the macros list the permissions each operation requires, using `ras-permission-manifest`.

### Identity and sessions

- `ras-identity-local`: username and password sign-in with Argon2id hashes, for users held in memory.
- `ras-identity-oauth2`: OAuth2 authorization-code sign-in with PKCE and state, and ID token claim validation.
- `ras-identity-session`: HMAC-signed JWT sessions with `iss` and `aud` checks, an in-memory session registry for revocation,
  and a `JwtAuthProvider` that the generated services accept.

### Observability

- `ras-observability-core` defines hooks for request counts and method durations.
- `ras-observability-otel` records them with OpenTelemetry and exports them for Prometheus.

### Supporting crates

- `ras-openrpc-types`: Rust types for the OpenRPC 1.3.2 specification, with builders and validation.
- `ras-transport-core`: the HTTP transport behind generated clients, using reqwest in production and axum-test in tests.
- `ras-spec-components` and `ras-api-explorer-assets` are used by the macros' generated code, not directly.
