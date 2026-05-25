# Rust Agent Stack Examples

This directory contains example applications demonstrating various features of the Rust Agent Stack.

Prerequisites:
- Rust 1.88 or newer for the Rust 2024 example crates
- Node.js 22.13 or newer only for `wasm-ui-demo`

## Overview

The examples are organized to showcase different aspects of the framework:
- **JSON-RPC services** with authentication and OpenRPC documentation
- **REST APIs** with OpenAPI generation
- **WebSocket-based bidirectional communication**
- **OAuth2 authentication flows**
- **WebAssembly UI applications**

## Examples

### Basic JSON-RPC (`basic-jsonrpc/`)

Demonstrates core JSON-RPC functionality with a simple task management service.

- **api/**: Shared API definitions using the `jsonrpc_service!` macro
- **service/**: HTTP server implementation with:
  - JWT authentication using local user provider
  - OpenTelemetry metrics integration
  - Prometheus metrics endpoint
  - OpenRPC document generation

**Quick Start:**
```bash
cargo run -p basic-jsonrpc-service --locked
# API available at http://localhost:3000
# Metrics at http://localhost:3000/metrics
```

### Bidirectional Chat (`bidirectional-chat/`)

Real-time chat application showcasing WebSocket-based bidirectional JSON-RPC.

- **api/**: Shared WebSocket RPC definitions using `jsonrpc_bidirectional_service!`
- **server/**: Chat server with:
  - Multi-room support
  - Message persistence
  - User presence tracking
  - Typing indicators
- **tui/**: Terminal UI client with ratatui interface

**Quick Start:**
```bash
# Terminal 1: Start server
cargo run -p bidirectional-chat-server --locked

# Terminal 2: Start TUI client
cargo run -p bidirectional-chat-tui --locked
```

### OAuth2 Demo (`oauth2-demo/`)

Full OAuth2 authentication flow implementation with Google as the provider.

- **api/**: OAuth2-protected API definitions
- **server/**: Runnable OAuth2 server with:
  - Authorization code flow with PKCE
  - State management for security
  - JWT session creation after successful auth
  - Static file serving for frontend
  - Role-based permissions

**Quick Start:**
```bash
# 1. Set up Google OAuth2 credentials at https://console.cloud.google.com/
# 2. Configure credentials in examples/oauth2-demo/server/.env
cargo run -p oauth2-demo-server --locked
# Open browser to http://localhost:3000
```

### File Service Example (`file-service-example/`)

Focused file upload/download service generated from the `file_service!` macro.

- Streaming upload and download endpoints
- Bearer-token authentication
- OpenAPI document generation
- Minimal single-crate setup for learning the file-service macro

**Quick Start:**
```bash
cargo run -p file-service-example --locked
# API available at http://localhost:3000
```

### File Service WASM (`file-service-wasm/`)

File-service example with a Rust backend and an OpenAPI-generated TypeScript
fetch-client usage sample.

- **file-service-api/**: Shared file-service definition
- **file-service-backend/**: Axum server with filesystem storage and OpenAPI output
- **typescript-example/**: Minimal TypeScript usage sample for a generated client

**Quick Start:**
```bash
cargo check -p file-service-backend --locked
```

### REST WASM Example (`rest-wasm-example/`)

Demonstrates the REST macro for building type-safe REST APIs with a
TypeScript usage sample for an OpenAPI-generated fetch client.

- OpenAPI 3.0 document generation
- Mock bearer-token authentication for protected routes
- CRUD operations for task management
- Request/response validation
- Minimal TypeScript usage sample for an OpenAPI-generated fetch client

**Quick Start:**
```bash
cargo check -p rest-backend --locked
```

### WASM UI Demo (`wasm-ui-demo/`)

Browser UI using the Dominator reactive framework and the generated JSON-RPC
client from the basic service example.

- Rust UI components styled with dwind
- Real-time task management
- Browser JSON-RPC client connected to `basic-jsonrpc-service`
- Reactive state management with futures-signals
- Dark theme support
- Responsive design

**Quick Start:**
```bash
# Terminal 1: Start the backend service
cargo run -p basic-jsonrpc-service --locked

# Terminal 2: Build and serve the WASM app
npm --prefix examples/wasm-ui-demo ci
npm --prefix examples/wasm-ui-demo start
# Open browser to http://localhost:8080
```

## Architecture Patterns

### Multi-Crate Examples
Examples like `basic-jsonrpc/`, `bidirectional-chat/`, `file-service-wasm/`,
`oauth2-demo/`, and `rest-wasm-example/` are structured as multi-crate workspaces:
- `api/`: Shared type definitions and service traits
- `server/`: Backend implementation
- `tui/`, `typescript-example/`, or a browser UI crate: Client-side example

This separation allows:
- Code reuse between client and server
- Independent versioning
- Clear API contracts

### Focused Single-Crate Examples
`file-service-example/` keeps a runnable service in one crate for a focused
file-macro demonstration. `wasm-ui-demo/` is a single frontend crate, but it
intentionally depends on `basic-jsonrpc-api` and expects
`basic-jsonrpc-service` to be running.

## Common Features

### Authentication
Most examples demonstrate authentication patterns:
- **Local users**: Username/password with Argon2 hashing
- **OAuth2**: External provider integration
- **JWT sessions**: Stateless authentication tokens
- **Permissions**: Role-based access control

### Observability
Examples include monitoring capabilities:
- OpenTelemetry integration
- Prometheus metrics export
- Structured logging with tracing

### Documentation
API documentation is auto-generated:
- **OpenRPC**: For JSON-RPC services
- **OpenAPI**: For REST APIs
- Generated at compile-time or runtime

## Development Tips

1. **Environment Variables**: Check `.env.example` files for required configuration
2. **Dependencies**: Examples use workspace dependencies from the root `Cargo.toml`
3. **Cross-Example Integration**: Some examples (like wasm-ui-demo) connect to other example services
4. **Generated Files**: OpenRPC/OpenAPI documents are typically generated in `target/` directories

## Testing

Examples use focused tests where they protect the demonstrated contract or
runtime behavior:
- Unit tests in the source files
- Integration tests in `tests/` directories
- Manual testing instructions in example-specific READMEs

Run the workspace test suite, including examples:
```bash
cargo test --workspace --all-targets --all-features --locked
```

Browser-facing examples are covered by the root CI as well:
- The Dominator WASM UI builds with `npm --prefix examples/wasm-ui-demo run build`.
- API explorer flows run under `tests/playwright`.
