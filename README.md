# Rust Agent Stack (RAS)

A Rust framework for building type-safe, authenticated agent systems with JSON-RPC, REST APIs, and file services.

## Overview

The Rust Agent Stack provides reusable building blocks for distributed agent systems:
- **Pluggable Authentication** - JWT sessions, OAuth2, local username/password auth, and reusable authorization traits
- **Type-Safe APIs** - Procedural macros for JSON-RPC, REST, and file services
- **WebSocket Support** - Bidirectional real-time communication
- **File Services** - Type-safe file upload/download with streaming support
- **Generated Clients** - Rust and browser-friendly clients generated from shared service contracts
- **Reactive WASM UIs** - Browser apps built with Dominator and generated API clients
- **Observability** - OpenTelemetry and Prometheus metrics hooks
- **API Documentation** - Automatic OpenRPC and OpenAPI generation
- **Compile-Time Safety** - Generated traits require every endpoint to be implemented

## Quick Start

Prerequisites:
- Rust 1.88 or newer for Rust 2024 edition crates
- Node.js 22.13 or newer only for the WASM UI example and Playwright browser tests

```bash
# Clone the repository
git clone https://github.com/JedimEmO/rust-agent-stack.git
cd rust-agent-stack

# Build the entire workspace with the checked-in lockfile
cargo build --locked

# Run an example service
cargo run -p basic-jsonrpc-service --locked
```

The service listens on `http://localhost:3000` with:

- JSON-RPC endpoint: `POST /rpc`
- Explorer UI: `http://localhost:3000/rpc/explorer`
- OpenRPC document: `http://localhost:3000/rpc/explorer/openrpc.json`
- Prometheus metrics: `http://localhost:3000/metrics`

Frontend examples are optional. The generated-client TypeScript samples are
plain usage files under `examples/*/typescript-example`, while the only
npm-based app is [`examples/wasm-ui-demo`](examples/wasm-ui-demo/).

## Architecture

RAS is organized as a Cargo workspace with the following structure:

```
crates/
├── core/                     # Core libraries
│   ├── ras-auth-core        # Authentication traits and types
│   ├── ras-identity-core    # Core identity provider traits
│   ├── ras-observability-core # Unified observability traits
│   └── ras-version-core     # API version migration traits
├── rpc/                     # JSON-RPC libraries
│   ├── ras-jsonrpc-types    # JSON-RPC 2.0 protocol types
│   ├── ras-jsonrpc-core     # JSON-RPC runtime support
│   ├── ras-jsonrpc-macro    # JSON-RPC service macro
│   └── bidirectional/       # WebSocket support
│       ├── ras-jsonrpc-bidirectional-types
│       ├── ras-jsonrpc-bidirectional-server
│       ├── ras-jsonrpc-bidirectional-client
│       └── ras-jsonrpc-bidirectional-macro
├── rest/                    # REST API libraries
│   ├── ras-rest-core        # REST types and utilities
│   ├── ras-rest-macro       # REST service macro
│   └── ras-file-macro       # File upload/download macro
├── identity/                # Identity providers
│   ├── ras-identity-local   # Username/password auth
│   ├── ras-identity-oauth2  # OAuth2 with PKCE support
│   └── ras-identity-session # JWT session management
├── observability/           # Monitoring and metrics
│   └── ras-observability-otel # OpenTelemetry implementation
├── specs/                   # Specification types
│   └── openrpc-types        # OpenRPC 1.3.2 spec types
examples/                    # Example applications
├── basic-jsonrpc/           # JSON-RPC service demo
├── bidirectional-chat/      # Real-time chat system
├── file-service-example/    # File upload/download demo
├── file-service-wasm/       # File service with TypeScript
├── oauth2-demo/             # OAuth2 authentication flow
├── rest-wasm-example/       # REST with TypeScript client
└── wasm-ui-demo/            # Dominator WASM UI
```

## Key Features

### Type-Safe JSON-RPC Services

Define services with compile-time type checking:

```rust
use ras_jsonrpc_macro::jsonrpc_service;

jsonrpc_service!({
    service_name: TaskService,
    openrpc: true,  // Generate OpenRPC docs
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) create_task(CreateTaskRequest) -> Task,
        WITH_PERMISSIONS(["admin"]) delete_all_tasks(()) -> (),
    ]
});

// Implement the generated `TaskServiceTrait` on your service type. The
// runnable task implementation lives in `examples/basic-jsonrpc/service`.
struct TaskServiceImpl;

// Use with the builder
let router = TaskServiceBuilder::new(TaskServiceImpl)
    .base_url("/rpc")
    .auth_provider(MyAuthProvider)
    .build()?;
```

### Type-Safe REST APIs

Build RESTful services with automatic OpenAPI documentation that can feed
TypeScript client generation:

```rust
use ras_rest_macro::rest_service;

rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,  // Serve the built-in API explorer at /api/v1/docs
    endpoints: [
        GET UNAUTHORIZED users() -> UsersResponse,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> UserResponse,
        GET WITH_PERMISSIONS(["user"]) users/{id: String}() -> User,
        DELETE WITH_PERMISSIONS(["admin"]) users/{id: String}() -> (),
    ]
});

// Implement the generated `UserServiceTrait` on your service type. The
// REST guide includes an in-memory implementation.
struct UserServiceImpl;

// Build the Axum router
let app = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(jwt_auth_provider)
    .build();
```

### Bidirectional WebSocket Communication

Real-time bidirectional messaging with authentication:

```rust
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;

jsonrpc_bidirectional_service!({
    service_name: ChatService,

    client_to_server: [
        WITH_PERMISSIONS(["user"]) send_message(SendMessageRequest) -> SendMessageResponse,
    ],

    server_to_client: [
        message_received(MessageReceivedNotification),
        user_joined(UserJoinedNotification),
    ],

    server_to_client_calls: [
    ]
});
```

### Type-Safe File Services

Build file upload/download services with streaming support:

```rust
use ras_file_macro::file_service;

file_service!({
    service_name: DocumentService,
    base_path: "/api/documents",
    body_limit: 52428800,  // 50MB
    endpoints: [
        UPLOAD WITH_PERMISSIONS(["user"]) upload() -> FileMetadata,
        DOWNLOAD UNAUTHORIZED download/{file_id: String}(),
    ]
});
```

### Generated Client Support

Service macros generate Rust clients and API documents from the same definitions used by the server. The REST and file-service browser examples are plain TypeScript usage samples that assume a fetch client generated locally from OpenAPI, while the JSON-RPC WASM UI uses generated Rust/WASM client code from the shared API crate.

```typescript
import * as api from './generated';

const users = await api.getUsers({
  baseUrl: 'http://localhost:3000/api/v1',
});

const created = await api.postUsers({
  baseUrl: 'http://localhost:3000/api/v1',
  headers: { Authorization: 'Bearer admintoken' },
  body: { name: 'Alice', email: 'alice@example.com' },
});
```

### Reactive WASM UIs

Build browser UIs with Dominator:

```rust
use dominator::{html, Dom};
use futures_signals::signal_vec::MutableVec;

fn create_task_list(tasks: MutableVec<Task>) -> Dom {
    html!("div", {
        .class("task-list")
        .children_signal_vec(tasks.signal_vec_cloned()
            .map(|task| render_task(task)))
    })
}
```

## Examples

### [Basic JSON-RPC](examples/basic-jsonrpc/)
Simple task management API demonstrating authentication and OpenTelemetry metrics.

### [OAuth2 Demo](examples/oauth2-demo/)
OAuth2 demo with PKCE flow, JWT sessions, and role-based permissions.

### [Bidirectional Chat](examples/bidirectional-chat/)
Real-time chat system with WebSocket communication, TUI client, and persistence.

### [File Service Example](examples/file-service-example/)
File upload/download service with streaming support and authentication.

### [File Service WASM](examples/file-service-wasm/)
File service with OpenAPI output and a minimal TypeScript usage sample for a generated fetch client.

### [REST WASM Example](examples/rest-wasm-example/)
REST API with OpenAPI output and a minimal TypeScript usage sample for a generated fetch client.

### [WASM UI Demo](examples/wasm-ui-demo/)
Reactive web UI with Dominator and the generated JSON-RPC client.

## Documentation

Detailed guides:
- [REST Macro Guide](documentation/ras-rest-macro.md) - REST API guide
- [File Service Guide](documentation/ras-file-macro.md) - File upload/download services
- [Identity Providers](documentation/ras-identity.md) - Authentication system guide
- [Observability](documentation/ras-observability.md) - Metrics and monitoring

Package-level guides:
- [JSON-RPC Macro](crates/rpc/ras-jsonrpc-macro/README.md) - JSON-RPC service generation, OpenRPC output, and generated clients
- [JSON-RPC Core](crates/rpc/ras-jsonrpc-core/README.md) - runtime auth and JSON-RPC support types
- [Bidirectional JSON-RPC Macro](crates/rpc/bidirectional/ras-jsonrpc-bidirectional-macro/README.md) - WebSocket service generation
- [Bidirectional JSON-RPC Server](crates/rpc/bidirectional/ras-jsonrpc-bidirectional-server/README.md) - server-side WebSocket runtime
- [Bidirectional JSON-RPC Client](crates/rpc/bidirectional/ras-jsonrpc-bidirectional-client/README.md) - native and WASM WebSocket clients

## Built-in Features

### Authentication & Security
- **Timing Attack Mitigation** - Missing local users verify against an Argon2 sentinel hash
- **Username Enumeration Mitigation** - Uniform invalid-credentials errors
- **Rate Limiting** - Local authentication limits concurrent verification attempts
- **Password Storage** - Per-user salted Argon2id hashes
- **JWT Configuration** - Configurable algorithms, secrets, TTLs, and active-session enforcement
- **PKCE OAuth2** - Proof Key for Code Exchange by default
- **Session Management** - JWT-based sessions with revocation support

### Observability

Add Prometheus-compatible metrics with minimal configuration:

```rust
use ras_observability_otel::standard_setup;

// Set up OpenTelemetry with Prometheus
let otel = standard_setup("my-service")?;

let _usage_tracker = otel.usage_tracker();
let _duration_tracker = otel.method_duration_tracker();
let _metrics_router = otel.metrics_router();

// Wire the trackers into generated service builders through their
// `with_usage_tracker` and `with_method_duration_tracker` hooks.

// Metrics available at /metrics endpoint
```

Features:
- Unified metrics for JSON-RPC, REST, and file services
- Request counting, duration tracking, user activity
- Prometheus exporter and Axum `/metrics` router helpers
- Extensible trait-based design

### TypeScript And WASM Support

Browser examples use generated contracts without hand-written DTOs:
- REST and file-service TypeScript usage samples assume a fetch client generated from OpenAPI specs.
- JSON-RPC WASM UI uses generated Rust/WASM client code from the shared API crate.
- Bearer tokens are passed as ordinary per-request headers.

## Development

Use the workspace commands below as the baseline development checks. They mirror
the GitHub Actions workflow so local validation catches the same classes of
breakage before a pull request.

### Rust Checks

```bash
# Formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Tests and doctests
cargo test --workspace --all-targets --all-features --no-run --locked
cargo test --workspace --all-targets --all-features --locked
cargo test --doc --workspace --all-features --locked

# Documentation
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

### Documentation Hygiene

CI also checks that each Cargo package has a local README target and that local
Markdown links resolve. The checks are implemented directly in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml) so the repository does
not need separate verification scripts.

### Supply Chain Policy

The tracked [`deny.toml`](deny.toml) is enforced in CI with `cargo-deny`.
Run the same check locally when dependency versions, features, or licenses
change:

```bash
cargo deny check
```

Install `cargo-deny` first if it is not already available:

```bash
cargo install cargo-deny
```

### Frontend Examples

```bash
# Generate OpenAPI specs used by the TypeScript usage samples
cargo check -p file-service-backend -p rest-backend --locked
```

The generated-client usage samples are plain TypeScript files:

- `examples/file-service-wasm/typescript-example/src/example.ts`
- `examples/rest-wasm-example/typescript-example/src/example.ts`

The only npm-based frontend example is the Dominator WASM UI:

```bash
# Dominator WASM UI
npm --prefix examples/wasm-ui-demo ci
npm --prefix examples/wasm-ui-demo run build
```

### Browser Explorer Tests

From the workspace root:

```bash
npm --prefix tests/playwright ci
npm --prefix tests/playwright run install:browsers
npm --prefix tests/playwright test
```

These tests start dedicated REST and JSON-RPC fixture servers and exercise the
generated API explorers in Chromium.

### Coverage

From the workspace root:

```bash
cargo llvm-cov --workspace --all-targets --all-features --locked --lcov --output-path lcov.info
cargo llvm-cov report --summary-only
```

Install `cargo-llvm-cov` first if it is not already available:

```bash
cargo install cargo-llvm-cov
```

## Contributing

Contributions are welcome. Keep changes focused, include tests for behavioral changes, and run the relevant workspace checks before opening a pull request.

### Development Setup

1. Install Rust 1.88 or newer
2. Install Node.js 22.13 or newer for the WASM UI example and Playwright browser tests
3. Clone the repository
4. Run `cargo build --locked` to verify setup

## License

This project is licensed under either MIT or Apache-2.0. See
[`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

## Acknowledgments

Built with these Rust crates:
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [Tokio](https://tokio.rs/) - Async runtime
- [Dominator](https://github.com/Pauan/rust-dominator) - WASM UI framework
- [Tungstenite](https://github.com/snapview/tungstenite-rs) - WebSocket implementation
- [hmac](https://github.com/RustCrypto/MACs), [sha2](https://github.com/RustCrypto/hashes), and [base64](https://github.com/marshallpierce/rust-base64) - HMAC-signed JWT support
- [async-trait](https://github.com/dtolnay/async-trait) - Async traits
