# Bidirectional Chat Server Integration Tests

This directory contains focused integration tests for the bidirectional chat server. The tests are organized into two files:

## Test Files

### `server_tests.rs`
Basic server functionality and configuration tests:
- **Configuration Tests**: Validates default values, configuration parsing, and validation logic
- **Example Config Test**: Loads `../config.example.toml` and verifies its JWT secret works with session creation
- **Persistence Tests**: Tests the persistence layer for storing and loading chat state
- **Server Lifecycle**: Tests in-memory server startup and health checks
- **Rate Limiting Configuration**: Validates rate limiting settings
- **CORS Configuration**: Tests CORS settings and validation
- **Logging Configuration**: Validates logging settings

### `auth_lifecycle_tests.rs`
Chat server auth and lifecycle tests:
- **Server Lifecycle**: Tests in-memory server startup and health checks
- **User Authentication**: Tests login with valid, invalid, and missing credentials
- **User Registration**: Tests new user registration, duplicate rejection, and login after registration
- **Admin Permissions**: Tests admin vs regular user permissions in JWT claims
- **Concurrent Users**: Tests multiple users logging in simultaneously

### `../src/main.rs` Unit Tests
Socketless WebSocket flow tests for the real chat server implementation:
- **Generated WebSocket Dispatch**: Runs the generated `ChatServiceHandler` through the in-memory `WebSocketIo` adapter
- **Room Join Flow**: Verifies an authenticated client can join the default room
- **Message Broadcast Flow**: Verifies `send_message` emits both the JSON-RPC response and `message_received` notification without binding a socket
- **Multi-user Broadcast Flow**: Verifies a message from one authenticated user is delivered to multiple room members through the in-memory connection manager
- **Room Presence Flow**: Verifies `list_rooms` user counts and `leave_room` notifications across multiple authenticated connections
- **Profile Management Flow**: Verifies profile read/update behavior and multi-word cat avatar persistence through the generated handler
- **Admin Operation Flow**: Verifies generated permission enforcement, moderator kicks, and admin announcements without binding a socket
- **Disconnect Cleanup Flow**: Verifies typing state, room membership, and user-left notifications are cleaned up on disconnect
- **Request Error Recovery Flow**: Verifies the in-memory WebSocket handler sends a JSON-RPC error response and continues processing later requests
- **Message Rate Limit Flow**: Verifies `messages_per_minute` rejects excess `send_message` calls without closing the in-memory connection

## Running the Tests

Run all tests:
```bash
cargo test -p bidirectional-chat-server --all-targets --all-features --locked
```

Run only server configuration tests:
```bash
cargo test -p bidirectional-chat-server --test server_tests --locked
```

Run only auth lifecycle tests:
```bash
cargo test -p bidirectional-chat-server --test auth_lifecycle_tests --locked
```

Run a specific test:
```bash
cargo test -p bidirectional-chat-server --locked test_user_authentication
```

## Test Coverage

The tests cover the following areas:

1. **Server Startup and Configuration**
   - Configuration file parsing and validation
   - Example config loading
   - Default value handling
   - Invalid configuration detection

2. **User Registration and Login**
   - New user registration
   - Duplicate username rejection
   - Login after registration
   - Login with credentials
   - Invalid credential handling
   - Concurrent user sessions

3. **Authorization**
   - Admin permission assignment
   - Regular user permission assignment
   - Permission-bearing JWT claims

4. **WebSocket Message Flow**
   - Socketless generated handler dispatch
   - Authenticated room join
   - Message response and notification emission
   - Multi-user room broadcast through the in-memory connection manager
   - Multi-user room list and leave presence updates
   - Profile update/readback with multi-word avatar fields
   - Moderator kick and admin announcement notifications
   - Typing-state and room cleanup on disconnect
   - Request error response followed by a successful request on the same in-memory connection
   - Per-user chat message rate limiting

5. **Persistence**
   - Message persistence to disk
   - Room state persistence
   - State recovery after restart

6. **Error Cases and Edge Conditions**
   - Invalid configuration values
   - Authentication failures
   - Concurrent access scenarios

## Test Architecture

The tests use in-memory harnesses:
- `server_tests.rs` keeps configuration, health, and persistence checks isolated from auth setup.
- `auth_lifecycle_tests.rs` runs HTTP-style requests through `axum-test` with login and registration wired through the same in-memory identity provider.
- The `../src/main.rs` WebSocket unit tests exercise the real `ChatServer` through the generated handler, in-memory socket adapter, and in-memory connection manager.
- Both suites use `axum-test` mock transport instead of binding sockets for HTTP checks.
- Both suites create temporary directories for runtime data and support concurrent test execution.

## Known Coverage Gaps

Areas for additional test coverage:
1. Deployment-level connection and login-attempt rate limiting hooks; configuration validation is covered, but the chat service currently enforces only per-user message limits
2. Reconnection behavior beyond disconnect cleanup
3. Performance and load testing
