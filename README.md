# Rust Agent Stack (RAS)

Rust Agent Stack is a set of Rust crates for building typed, authenticated
HTTP and WebSocket APIs on Axum.
You declare an API once, and a macro generates the server, the clients,
the API documents and the auth checks from that declaration.

Guide: <https://jedimemo.github.io/rust-api-stack/>

## The problem

A hand-written API keeps the same facts in several places.
The handler decides what a request looks like, the client guesses it,
the OpenAPI file describes it, and the auth middleware decides who may call it.
Each copy can drift from the others,
and the drift shows up at runtime, often in someone else's client.

RAS keeps those facts in one Rust declaration.
The handler trait, the client, the API document and the permission check are all generated from it,
so a change to the contract is a compile error wherever the code no longer fits.

## What it looks like

A JSON-RPC service with one public method and one protected method:

```rust
use ras_jsonrpc_core::AuthenticatedUser;
use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateTaskRequest {
    pub title: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Task {
    pub title: String,
    pub owner: String,
}

jsonrpc_service!({
    service_name: TaskService,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED ping(()) -> String,
        WITH_PERMISSIONS(["task:write"]) create_task(CreateTaskRequest) -> Task,
    ]
});

struct Tasks;

impl TaskServiceTrait for Tasks {
    async fn ping(&self, _: ()) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("pong".into())
    }

    async fn create_task(
        &self,
        user: &AuthenticatedUser,
        request: CreateTaskRequest,
    ) -> Result<Task, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Task { title: request.title, owner: user.user_id.clone() })
    }
}

let app = TaskServiceBuilder::new(Tasks)
    .base_url("/rpc")
    .auth_provider(my_auth_provider)
    .build()?;
```

Notice that `create_task` receives an `&AuthenticatedUser`.
The generated server only calls it after your auth provider has accepted the bearer token
and the user holds the `task:write` permission; `ping` needs no credential.
The same declaration also generates a `TaskServiceClient`,
an OpenRPC document, and an API explorer page at `/rpc/explorer`.

There is one macro for each kind of API:

| Macro | Crate | For |
| --- | --- | --- |
| `jsonrpc_service!` | `ras-jsonrpc-macro` | JSON-RPC over HTTP, with OpenRPC output and an API explorer |
| `rest_service!` | `ras-rest-macro` | JSON REST APIs, with OpenAPI output and an API explorer |
| `file_service!` | `ras-file-macro` | Streaming multipart uploads and ranged downloads, with OpenAPI output |
| `jsonrpc_bidirectional_service!` | `ras-jsonrpc-bidirectional-macro` | JSON-RPC in both directions over a WebSocket |

All four share one auth syntax: `UNAUTHORIZED`, `OPTIONAL_AUTH` and `WITH_PERMISSIONS([...])`.
The JSON-RPC and REST macros also accept typed request and response examples,
which appear in the generated document and in the explorer.

## Getting it

Add the macro crate and the runtime crates its generated code refers to.
For the JSON-RPC example above, in a single crate:

```toml
[dependencies]
ras-jsonrpc-macro = "0.4"
ras-jsonrpc-core = "0.3"
ras-jsonrpc-types = "0.3"
ras-transport-core = { version = "0.2", features = ["reqwest"] }
axum = "0.8"
schemars = "1.2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

The macro's default features generate both the server and a reqwest-based client.
A real application usually puts the declarations in a shared API crate,
with `server` and `client` features so a browser client does not compile Axum.
The [tutorial](https://jedimemo.github.io/rust-api-stack/tutorial/index.html) builds that layout,
and each macro guide lists the dependencies for its macro.

The crates need Rust 1.88 or newer.

### The other crates

| Crate | Purpose |
| --- | --- |
| `ras-auth-core` | The `AuthProvider` trait, `AuthenticatedUser`, and bearer, cookie and CSRF transport |
| `ras-identity-core` | Traits for identity providers |
| `ras-identity-local` | Username and password sign-in with Argon2 hashes |
| `ras-identity-oauth2` | OAuth2 sign-in with PKCE |
| `ras-identity-session` | JWT sessions, and an `AuthProvider` that verifies them |
| `ras-observability-core` | Traits for request metrics and usage tracking |
| `ras-observability-otel` | OpenTelemetry and Prometheus implementation of those traits |
| `ras-transport-core` | The HTTP transport behind generated clients |
| `ras-version-core` | Traits for migrating versioned request and response shapes |
| `ras-jsonrpc-core`, `ras-jsonrpc-types` | Runtime support and protocol types for `jsonrpc_service!` |
| `ras-rest-core`, `ras-file-core` | Runtime types for `rest_service!` and `file_service!` |
| `ras-jsonrpc-bidirectional-server`, `-client`, `-types` | WebSocket runtime for `jsonrpc_bidirectional_service!`; the client runs natively and in WASM |
| `ras-openrpc-types` | Rust types for the OpenRPC 1.3.2 specification |
| `ras-permission-manifest` | A machine-readable list of the permissions each service requires |
| `ras-api-explorer-assets`, `ras-spec-components` | Explorer page and document-building code that the macros embed; not used directly |

## Running an example

Clone the repository and run the JSON-RPC example:

```bash
git clone https://github.com/JedimEmO/rust-api-stack.git
cd rust-api-stack
cargo run -p basic-jsonrpc-service --locked
```

The service listens on `http://localhost:3000`.
The JSON-RPC endpoint is `POST /rpc`, the explorer is at `/rpc/explorer`,
the OpenRPC document at `/rpc/explorer/openrpc.json`, and Prometheus metrics at `/metrics`.

The other examples:

| Example | Shows |
| --- | --- |
| [`basic-jsonrpc`](examples/basic-jsonrpc/) | JSON-RPC task service with a demo sign-in, permission checks and OpenTelemetry metrics |
| [`oauth2-demo`](examples/oauth2-demo/) | OAuth2 sign-in with PKCE, JWT sessions and role-based permissions |
| [`bidirectional-chat`](examples/bidirectional-chat/) | WebSocket chat server with a terminal client and persistence |
| [`file-service-example`](examples/file-service-example/) | File uploads and downloads with streaming and auth |
| [`file-service-wasm`](examples/file-service-wasm/) | File service OpenAPI output and a TypeScript client generated from it |
| [`rest-wasm-example`](examples/rest-wasm-example/) | REST OpenAPI output and a TypeScript client generated from it |
| [`wasm-ui-demo`](examples/wasm-ui-demo/) | Browser UI in Rust with Dominator and the generated JSON-RPC client |

The TypeScript samples are plain usage files.
Only `wasm-ui-demo` uses npm, and it needs Node.js 22.13 or newer.

## Documentation

The [guide](https://jedimemo.github.io/rust-api-stack/) is the manual.
Its source is in [`documentation/src`](documentation/src/SUMMARY.md). Good places to start:

- [Build a typed workspace app](documentation/src/tutorial/index.md), a tutorial from contract to clients and tests
- [Why typed service definitions](documentation/src/why-typed-service-definitions.md)
- [Auth in the API contract](documentation/src/auth-in-api-contract.md)
- The macro guides for [`jsonrpc_service!`](documentation/src/macros/jsonrpc-service.md), [`rest_service!`](documentation/src/macros/rest-service.md), [`file_service!`](documentation/src/macros/file-service.md) and [`jsonrpc_bidirectional_service!`](documentation/src/macros/bidirectional-jsonrpc-service.md)
- [Identity and sessions](documentation/src/identity-and-sessions.md) and [Observability](documentation/src/observability.md)

Each crate's README covers its own API.

## Development

These commands mirror the CI workflow in [`.github/workflows/ci.yml`](.github/workflows/ci.yml):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-targets --all-features --locked
cargo test --doc --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo deny check
mdbook build
```

Tests run under [cargo-nextest](https://nexte.st) with retries disabled (see `.config/nextest.toml`),
because a test that only passes on a retry is a bug in the test.
A test that hangs fails after two minutes locally and three in CI.
A weekly `Flake detector` workflow runs the whole suite five times at three parallelism levels,
and it can also be started by hand from the Actions tab.

Coverage uses `cargo-llvm-cov`, and CI posts the same table on every pull request:

```bash
cargo llvm-cov nextest --workspace --all-targets --all-features --locked
```

The API explorer has browser tests that start REST and JSON-RPC fixture servers and drive them in Chromium:

```bash
npm --prefix tests/playwright ci
npm --prefix tests/playwright run install:browsers
npm --prefix tests/playwright test
```

The TypeScript samples read OpenAPI files that their backends write at build time,
so build those first with `cargo check -p file-service-backend -p rest-backend --locked`.

Contributions are welcome.
Keep changes focused, include tests for behaviour changes,
and run the checks above before opening a pull request.

## License

Licensed under either MIT or Apache-2.0, at your option.
See [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE).

## Acknowledgments

Built with these Rust crates:

- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [Tokio](https://tokio.rs/) - Async runtime
- [Dominator](https://github.com/Pauan/rust-dominator) - WASM UI framework
- [Tungstenite](https://github.com/snapview/tungstenite-rs) - WebSocket implementation
- [hmac](https://github.com/RustCrypto/MACs), [sha2](https://github.com/RustCrypto/hashes), and [base64](https://github.com/marshallpierce/rust-base64) - HMAC-signed JWT support
- [async-trait](https://github.com/dtolnay/async-trait) - Async traits
