# REST API contract

The contract crate for the [REST example](../README.md).
It defines the request and response types and calls `rest_service!` to declare `UserService` at `/api/v1`.
[`rest-backend`](../rest-backend/README.md) implements it,
and [`typescript-example`](../typescript-example/README.md) calls it through a generated client.

## What the declaration shows

[src/lib.rs](src/lib.rs) uses most of the endpoint syntax:

- path parameters, as in `users/{id: String}` and `users/{user_id: String}/tasks/{task_id: String}`
- query parameters, as in `search/users ? q: String & limit: Option<u32> & offset: Option<u32>`
- public endpoints with `UNAUTHORIZED` and protected ones with `WITH_PERMISSIONS(["admin"])` or `WITH_PERMISSIONS(["user"])`
- doc comments, which become the operation summaries
- authored examples on `GET users/{id}`, one success and one 404 error, and on `GET search/users`
- `serve_docs: true` with `docs_path: "/docs"`, which serves the API explorer and the OpenAPI document at `/api/v1/docs`

The example ids and names in the authored examples are illustrative;
they do not match the backend's demo data.

## Features

- `server` generates the server trait, the router builder, the explorer routes and the OpenAPI functions.
- `client` generates the Rust client with the default reqwest transport.
- With no features, the crate holds only the shared types.

## Checks

From the workspace root:

```bash
cargo check -p rest-api --locked
cargo check -p rest-api --features server --locked
cargo check -p rest-api --features client --locked
cargo test -p rest-api --features server --locked
cargo clippy -p rest-api --all-targets --all-features --locked -- -D warnings
```

The handler tests live in the backend:

```bash
cargo test -p rest-backend --locked
```
