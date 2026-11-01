# Build a typed workspace app

This tutorial designs a small application with RAS,
from the first API boundary decision to clients, tests and deployment wiring.

The example application is a small team workspace:

- users list projects and tasks;
- users create and update tasks;
- users upload and download task attachments;
- clients can receive live activity notifications;
- admins can perform wider maintenance operations.

The domain matters less than the shape.
The API contract lives in a shared Rust crate,
that crate's `server` and `client` features forward to the service macro features,
and auth requirements sit beside the operation definitions.

## Target architecture

Use a workspace with clear crate boundaries:

```text
team-workspace/
  Cargo.toml
  crates/
    workspace-api/      # DTOs and service macro declarations
    workspace-server/   # Axum server, storage, auth provider, service impls
    workspace-web/      # optional Rust/WASM client
  web/                  # optional TypeScript app generated from OpenAPI
```

The `workspace-api` crate is the center. Server and client crates depend on it
with different features:

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["server"] }
```

```toml
[dependencies]
workspace-api = { path = "../workspace-api", default-features = false, features = ["client"] }
```

This keeps generated transport code out of crates that do not need it,
while the request and response types stay shared.
The macro crate's features decide what code is emitted;
the API crate's features are only the switch that downstream crates flip.
For the HTTP service macros, the macro crate's `client` feature emits clients
that take an injected transport,
and its `reqwest` feature also emits the default `build()` constructor.

## What you will build

By the end of the tutorial you will have:

- a typed API crate with REST, file, and optional WebSocket contracts;
- explicit permission requirements in the API definitions;
- an Axum server that implements generated traits;
- generated OpenAPI documents with request examples, and generated Rust clients;
- checks that prove no-default, server-only, client-only, and WASM builds keep
  working;
- a strategy for evolving the API without silently breaking clients.

The tutorial uses REST for the project and task workflows,
a file service for attachments, and bidirectional JSON-RPC for live notifications.
If your application is more command-oriented, the same structure works with
[`jsonrpc_service!`](../macros/jsonrpc-service.md) instead of
[`rest_service!`](../macros/rest-service.md).
