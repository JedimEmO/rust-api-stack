# Bidirectional Chat Server

Authenticated Axum server for the [bidirectional chat example](../README.md). It combines:

- REST auth endpoints generated from `bidirectional-chat-api::auth`
- JWT sessions from `ras-identity-session`
- Local username/password identity from `ras-identity-local`
- Bidirectional JSON-RPC over WebSocket at `/ws`
- File-backed chat room, message, and profile persistence

## Run

From the workspace root:

```bash
CHAT_CONFIG_FILE=examples/bidirectional-chat/server/config.example.toml \
CHAT_DATA_DIR=examples/bidirectional-chat/server/chat_data \
cargo run -p bidirectional-chat-server --locked
```

The example config binds to `127.0.0.1:3000`.

- HTTP base URL: `http://127.0.0.1:3000`
- WebSocket URL: `ws://127.0.0.1:3000/ws`
- Runtime data: `examples/bidirectional-chat/server/chat_data/`

`chat_data/` is ignored by git. The server also loads `.env` from the current working directory before reading configuration.

## Configuration

The server reads `config.toml` by default. Set `CHAT_CONFIG_FILE` to use another file:

```bash
CHAT_CONFIG_FILE=examples/bidirectional-chat/server/config.example.toml \
cargo run -p bidirectional-chat-server --locked
```

Direct environment overrides supported by the server:

- `HOST`
- `PORT`
- `JWT_SECRET`
- `CHAT_DATA_DIR`
- `RUST_LOG`

Nested config values use the `CHAT__SECTION__FIELD` form, for example:

```bash
CHAT__RATE_LIMIT__ENABLED=true \
CHAT__RATE_LIMIT__MESSAGES_PER_MINUTE=10 \
cargo run -p bidirectional-chat-server --locked
```

The checked-in [config.example.toml](config.example.toml) shows every supported section. Replace the example `jwt_secret` and admin passwords for any shared environment.

## Auth Endpoints

The REST endpoints are generated from the shared API crate and mounted at the root:

- `GET /health`
- `POST /auth/register`
- `POST /auth/login`

Register a user:

```bash
curl -fsS -X POST http://127.0.0.1:3000/auth/register \
  -H 'content-type: application/json' \
  --data '{"username":"demo","password":"demo12345","email":"demo@example.com","display_name":"Demo User"}'
```

Login:

```bash
curl -fsS -X POST http://127.0.0.1:3000/auth/login \
  -H 'content-type: application/json' \
  --data '{"username":"demo","password":"demo12345"}'
```

Debug builds also create `alice` / `alice123` and `bob` / `bob123`. When `config.example.toml` is loaded, it also creates the configured `admin` / `admin123456` and `moderator` / `moderator123` users.

## WebSocket Auth

The `/ws` endpoint requires a valid JWT. Clients can pass the token as:

- `Authorization: Bearer <token>`
- `x-auth-token: <token>`
- `sec-websocket-protocol: token.<token>`

Once connected, the generated chat service handles methods such as `join_room`, `send_message`, `list_rooms`, `update_profile`, `kick_user`, and `broadcast_announcement`.

## Tests

Run the server package tests:

```bash
cargo test -p bidirectional-chat-server --locked
```

Focused suites:

```bash
cargo test -p bidirectional-chat-server --test server_tests --locked
cargo test -p bidirectional-chat-server --test auth_lifecycle_tests --locked
```

HTTP tests use `axum-test` mock transport. WebSocket flow tests use the in-memory `WebSocketIo` adapter instead of binding sockets. See [tests/README.md](tests/README.md) for the current coverage map and known gaps.

## Checks

```bash
cargo test -p bidirectional-chat-server --locked
cargo clippy -p bidirectional-chat-server --all-targets --all-features --locked -- -D warnings
```
