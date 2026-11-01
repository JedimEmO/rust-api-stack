# Bidirectional chat server

The Axum server for the [bidirectional chat example](../README.md). It combines:

- REST login and registration endpoints generated from `bidirectional_chat_api::auth`
- JWT sessions from `ras-identity-session`
- in-memory username and password accounts from `ras-identity-local`
- bidirectional JSON-RPC over a WebSocket at `/ws`
- rooms, messages and profiles saved as files under the data directory

## Run

From the workspace root:

```bash
CHAT_CONFIG_FILE=examples/bidirectional-chat/server/config.example.toml \
CHAT_DATA_DIR=examples/bidirectional-chat/server/chat_data \
cargo run -p bidirectional-chat-server --locked
```

The server listens on `127.0.0.1:3000`:

- HTTP base URL: `http://127.0.0.1:3000`
- WebSocket URL: `ws://127.0.0.1:3000/ws`
- Runtime data: `examples/bidirectional-chat/server/chat_data/`, which git ignores

Without `CHAT_CONFIG_FILE` the server looks for `config.toml` in the current directory and otherwise uses built-in defaults.
It also loads a `.env` file from the current directory before it reads the configuration.

## Accounts

Debug builds create `alice` / `alice123` and `bob` / `bob123`.
With `config.example.toml` loaded, the server also creates `admin` / `admin123456`
with the `admin`, `moderator` and `user` permissions,
and `moderator` / `moderator123` with `moderator` and `user`.
Every other account gets `user`.

Accounts are kept in memory and are lost when the server stops.

## Configuration

These environment variables override the configuration file directly:

- `HOST`, an IP address such as `0.0.0.0`, and `PORT`
- `JWT_SECRET`, at least 32 bytes and free of placeholder words; generate one with `openssl rand -hex 32`
- `CHAT_DATA_DIR`
- `RUST_LOG`, a plain level such as `debug` or a filter such as `info,ras_jsonrpc_bidirectional_server=debug`

Other values use the `CHAT__SECTION__FIELD` form:

```bash
CHAT__RATE_LIMIT__ENABLED=true \
CHAT__RATE_LIMIT__MESSAGES_PER_MINUTE=10 \
cargo run -p bidirectional-chat-server --locked
```

Lists, such as the `[[admin.users]]` entries, cannot be set this way; put them in the configuration file.
[config.example.toml](config.example.toml) shows every section.
Replace its `jwt_secret` and passwords for anything other than local development.

With rate limiting enabled, the server enforces `messages_per_minute` on `send_message`.
It validates `connections_per_ip` and `login_attempts_per_hour` but does not enforce them;
they are left for a proxy or the deployment to apply.

## Auth endpoints

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

Sign in:

```bash
curl -fsS -X POST http://127.0.0.1:3000/auth/login \
  -H 'content-type: application/json' \
  --data '{"username":"demo","password":"demo12345"}'
```

The response holds the JWT in `token`, its expiry in `expires_at` and the user id in `user_id`.

## WebSocket authentication

The `/ws` endpoint requires a valid JWT. A client can send it in any of these headers:

- `Authorization: Bearer <token>`
- `x-auth-token: <token>`
- `Sec-WebSocket-Protocol: ras-jsonrpc, token.<token>`, for browsers, which cannot set headers on a WebSocket

## Tests

```bash
cargo test -p bidirectional-chat-server --locked
cargo test -p bidirectional-chat-server --test server_tests --locked
cargo test -p bidirectional-chat-server --test auth_lifecycle_tests --locked
cargo clippy -p bidirectional-chat-server --all-targets --all-features --locked -- -D warnings
```

Most tests run without a network socket:
HTTP tests use the `axum-test` mock transport,
and the WebSocket flow tests use the in-memory `WebSocketIo` adapter.
One router-level test binds a local port to drive the real `/ws` endpoint.
[tests/README.md](tests/README.md) maps the coverage.
