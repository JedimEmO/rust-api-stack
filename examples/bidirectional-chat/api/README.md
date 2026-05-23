# Bidirectional Chat API

Shared contract crate for the [bidirectional chat example](../README.md). It contains the JSON-RPC request/response types, server-to-client notification payloads, REST auth payloads, and generated service code used by the chat server and TUI client.

## Generated APIs

The WebSocket JSON-RPC contract in [src/lib.rs](src/lib.rs) generates `ChatService` with authenticated methods for:

- sending messages and typing notifications
- joining, leaving, and listing rooms
- reading and updating user profiles
- moderator kicks
- admin announcements

The REST auth contract in [src/auth.rs](src/auth.rs) generates:

- `GET /health`
- `POST /auth/register`
- `POST /auth/login`

The runnable server is documented in [../server/README.md](../server/README.md), and the terminal client is documented in [../tui/README.md](../tui/README.md).

## Features

- `server` - enables generated server integration and Axum support.
- `client` - enables the generated bidirectional client.
- The default feature set enables both.

## Checks

```bash
cargo check -p bidirectional-chat-api --locked
cargo check -p bidirectional-chat-api --no-default-features --features client --locked
cargo test -p bidirectional-chat-api --locked
cargo test -p bidirectional-chat-api --no-default-features --features client --locked
cargo clippy -p bidirectional-chat-api --all-targets --all-features --locked -- -D warnings
```
