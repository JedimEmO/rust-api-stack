# REST API Contract Example

Shared REST API contract for the REST/OpenAPI TypeScript usage sample.

This crate defines the DTOs and `rest_service!` declaration used by:

- `rest-backend`: Axum server implementation
- `../typescript-example/src/example.ts`: generated-client usage sample

## Generated API

The service declaration in `src/lib.rs` generates:

- Server trait and Axum router builder with the `server` feature
- Rust client helpers with the `client` feature
- OpenAPI document generation for TypeScript client generators
- Built-in API explorer routes when served by the backend

## Features

- `server`: enables generated server-side types and router integration
- `client`: enables generated Rust client helpers
- default: no generated server or client transport code

## Checks

From the workspace root:

```bash
cargo check -p rest-api --locked
cargo check -p rest-api --features server --locked
cargo check -p rest-api --features client --locked
cargo test -p rest-api --features server --locked
cargo clippy -p rest-api --all-targets --all-features --locked -- -D warnings
```

The backend package runs the server-side implementation tests:

```bash
cargo test -p rest-backend --locked
```

## Related Files

- `../rest-backend/README.md` - runnable backend, demo tokens, and endpoint map
- `../typescript-example/README.md` - minimal generated TypeScript client usage
