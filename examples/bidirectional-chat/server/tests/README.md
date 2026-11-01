# Bidirectional chat server tests

Integration tests for the [chat server](../README.md).
They call `build_application` with explicit configuration, an in-memory identity provider
and a temporary data directory, so they run in parallel without touching your local `chat_data`.

## `server_tests.rs`

Configuration, persistence and startup:

- default values, configuration validation and invalid values
- loading `../config.example.toml` and creating a session service with its JWT secret
- room, admin, rate-limit, CORS and logging settings
- the persistence layer: saving and reloading server state and room messages
- startup and the health check through the `axum-test` mock transport
- one router-level test that binds a local port, registers and signs in over REST,
  connects to `/ws` with the generated client, and checks persisted messages and disconnect cleanup

## `auth_lifecycle_tests.rs`

Login, registration and permissions through the real REST handlers:

- login with valid, invalid and missing credentials
- registration, duplicate usernames and login after registration
- admin and regular-user permissions in the JWT claims
- several users signing in at the same time

## WebSocket flow tests

The unit tests in [`../src/chat/tests.rs`](../src/chat/tests.rs) run the generated `ChatServiceHandler`
against the real `ChatServer`, through the in-memory `WebSocketIo` adapter and connection manager.
No socket is bound. They cover:

- joining a room and broadcasting a message to every member
- `list_rooms` user counts and `leave_room` notifications
- reading and updating profiles, including multi-word cat avatar values
- permission checks, moderator kicks and admin announcements
- cleaning up typing state and room membership on disconnect
- an error response followed by a successful request on the same connection
- the per-user `messages_per_minute` limit

## Running them

```bash
cargo test -p bidirectional-chat-server --all-targets --all-features --locked
cargo test -p bidirectional-chat-server --test server_tests --locked
cargo test -p bidirectional-chat-server --test auth_lifecycle_tests --locked
cargo test -p bidirectional-chat-server --locked test_user_authentication
```

## Not covered

- connection and login-attempt rate limits, which the server validates but does not enforce
- reconnecting after a disconnect
- performance and load
