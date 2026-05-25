# Build A Typed Workspace App

This tutorial walks through designing an application with RAS from the first API
boundary decision to clients, tests, and deployment wiring.

The example application is a small team workspace:

- users list projects and tasks;
- users create and update tasks;
- users upload and download task attachments;
- clients can receive live activity notifications;
- admins can perform wider maintenance operations.

The important part is not the domain. The important part is the shape: the API
contract lives in a shared Rust crate, generated server code is enabled by the
server feature, generated client code is enabled by the client feature, and auth
requirements are declared beside the operation definitions.

## Target Architecture

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

This keeps generated transport code out of crates that do not need it, while
keeping request and response types shared.

## What You Will Build

By the end of the tutorial you will have:

- a typed API crate with REST, file, and optional WebSocket contracts;
- explicit permission requirements in the API definitions;
- an Axum server that implements generated traits;
- generated OpenAPI and Rust client usage;
- checks that prove no-default, server-only, client-only, and WASM builds keep
  working;
- a practical strategy for evolving the API without silently breaking clients.

The tutorial uses REST for project/task workflows, file services for
attachments, and bidirectional JSON-RPC for live notifications. If your
application is more command-oriented, the same structure works with
[`jsonrpc_service!`](../macros/jsonrpc-service.md) instead of
[`rest_service!`](../macros/rest-service.md).

