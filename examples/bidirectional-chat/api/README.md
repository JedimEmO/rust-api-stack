# Bidirectional chat API

The contract crate for the [bidirectional chat example](../README.md).
It holds the WebSocket JSON-RPC contract, the REST login and registration contract,
and the request, response and notification types that the server and the terminal client share.

## Generated APIs

[src/lib.rs](src/lib.rs) declares `ChatService` with `jsonrpc_bidirectional_service!`.
Client-to-server methods cover messages, typing indicators, rooms, profiles,
moderator kicks and admin announcements; every one requires a permission.
Server-to-client notifications cover new messages, users joining and leaving, typing,
announcements, kicks and room changes.
The [example README](../README.md#the-protocol) lists them with their permissions.

[src/auth.rs](src/auth.rs) declares `ChatAuthService` with `rest_service!`:

- `GET /health`
- `POST /auth/register`
- `POST /auth/login`

The server is documented in [../server/README.md](../server/README.md)
and the terminal client in [../tui/README.md](../tui/README.md).

## Features

Both services use `feature_gated: true`, so this crate's features choose what is generated:

- `server` generates the WebSocket handler and the REST router builder.
- `client` generates the WebSocket client and the REST client, with the default reqwest transport.
- With no features, neither side is generated and the crate holds the shared types.

## Checks

```bash
cargo check -p bidirectional-chat-api --locked
cargo check -p bidirectional-chat-api --no-default-features --features server --locked
cargo check -p bidirectional-chat-api --no-default-features --features client --locked
cargo test -p bidirectional-chat-api --locked
cargo test -p bidirectional-chat-api --no-default-features --features server --locked
cargo test -p bidirectional-chat-api --no-default-features --features client --locked
cargo clippy -p bidirectional-chat-api --all-targets --all-features --locked -- -D warnings
```
