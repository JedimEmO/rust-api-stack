# Bidirectional chat example

A chat server and a terminal client that share one WebSocket connection for JSON-RPC in both directions.
The client calls methods on the server, such as `send_message` and `join_room`,
and the server pushes notifications back, such as `message_received` and `user_joined`.
Both sides come from one `jsonrpc_bidirectional_service!` declaration.

The example has three crates:

- [`api/`](api/README.md) declares the WebSocket contract and the REST login and registration contract.
- [`server/`](server/README.md) is the Axum server, with local accounts, JWT sessions and rooms, messages and profiles saved to disk.
- [`tui/`](tui/README.md) is a Ratatui terminal client built on the generated client.

## Run

Start the server:

```bash
cargo run -p bidirectional-chat-server --locked
```

It listens on `127.0.0.1:3000`, with the WebSocket endpoint at `ws://127.0.0.1:3000/ws`.
Chat state is saved to `./chat_data` under the directory you start it from;
set `CHAT_DATA_DIR` to put it elsewhere.

In another terminal, start the client:

```bash
cargo run -p bidirectional-chat-tui --locked
```

Debug builds of the server create two accounts, `alice` / `alice123` and `bob` / `bob123`.
To make a new account, press Ctrl+R on the login screen.
Accounts live in memory, so registered users are gone when the server stops;
rooms, messages and profiles are kept in `chat_data`.

Run the client in two terminals and sign in as `alice` and `bob` to see messages and typing indicators arrive in both.
The [client README](tui/README.md) lists the keys.

### Admin and moderator accounts

Start the server with the example configuration to also create `admin` / `admin123456` and `moderator` / `moderator123`:

```bash
CHAT_CONFIG_FILE=examples/bidirectional-chat/server/config.example.toml \
CHAT_DATA_DIR=examples/bidirectional-chat/server/chat_data \
cargo run -p bidirectional-chat-server --locked
```

The terminal client has no commands for kicking users or sending announcements.
It shows announcements, but you call `kick_user` and `broadcast_announcement` from your own client.

## The protocol

Client-to-server methods and the permission each one requires:

| Method | Permission |
|---|---|
| `send_message`, `join_room`, `leave_room`, `list_rooms` | `user` |
| `get_profile`, `update_profile` | `user` |
| `start_typing`, `stop_typing` | `user` |
| `kick_user` | `moderator` |
| `broadcast_announcement` | `admin` |

Server-to-client notifications:
`message_received`, `user_joined`, `user_left`, `user_started_typing`, `user_stopped_typing`,
`system_announcement`, `user_kicked`, `room_created` and `room_deleted`.
`join_room` creates the room when it does not exist and sends `room_created`.

Every registered user gets the `user` permission.
Accounts listed under `[admin]` in the configuration get the permissions listed there.

## Signing in

1. The client sends the username and password to `POST /auth/login` and receives a JWT.
2. The client opens `/ws` with the JWT in the `Authorization: Bearer` header.
3. The server checks the JWT during the WebSocket upgrade and rejects the connection without one.

Sessions are held in server memory, so a restart signs every client out.

## Configuration

The server reads `config.toml` from the directory it starts in, or the file in `CHAT_CONFIG_FILE`.
Environment variables override the file.
[server/README.md](server/README.md) lists them,
and [config.example.toml](server/config.example.toml) shows every section.

`JWT_SECRET` must be at least 32 bytes and must not contain placeholder words such as `secret` or `password`.
Generate one with `openssl rand -hex 32`.
Release builds refuse to start with the built-in development secret.
