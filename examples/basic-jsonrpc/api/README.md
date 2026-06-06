# Basic JSON-RPC API

Shared API contract for the [basic JSON-RPC example](../README.md). This crate defines the request and response types and invokes `ras-jsonrpc-macro` to generate the `MyService` server trait, router builder, optional client, OpenRPC document, and explorer routes.

## Generated Service

The contract is defined in [src/lib.rs](src/lib.rs). It includes:

- `sign_in`, `sign_out`, and `delete_everything`
- task CRUD methods
- profile read/update methods
- dashboard statistics

The service route is selected by the server crate when it builds the generated router. See [../service/README.md](../service/README.md) for the runnable server.

## Features

- `server` - enables generated server-side types and Axum integration.
- `client` - enables the generated HTTP client with the default reqwest transport.
- default: no generated transport code.

## Checks

```bash
cargo check -p basic-jsonrpc-api --locked
cargo check -p basic-jsonrpc-api --features server --locked
cargo check -p basic-jsonrpc-api --features client --locked
cargo test -p basic-jsonrpc-api --locked
cargo test -p basic-jsonrpc-api --features server --locked
cargo test -p basic-jsonrpc-api --features client --locked
cargo clippy -p basic-jsonrpc-api --all-targets --all-features --locked -- -D warnings
```
