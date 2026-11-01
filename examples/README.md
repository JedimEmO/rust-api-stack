# Rust API Stack examples

Runnable applications that use the Rust API Stack macros and crates.
Each example has its own README with the run command, the demo credentials and what to try.
For the concepts behind them, read the [mdBook guide](https://jedimemo.github.io/rust-api-stack/).

You need Rust 1.88 or newer, because the example crates use the 2024 edition.
`wasm-ui-demo` also needs Node.js 22.13 or newer.
Run every command from the repository root.

Every server binds port 3000,
so run one at a time unless you change the port where the server allows it.

## The examples

| Example | What it shows | Start it with |
|---|---|---|
| [basic-jsonrpc](basic-jsonrpc/) | `jsonrpc_service!` with permissions, authored OpenRPC examples, the API explorer and Prometheus metrics | `cargo run -p basic-jsonrpc-service --locked` |
| [bidirectional-chat](bidirectional-chat/) | `jsonrpc_bidirectional_service!` over a WebSocket, local accounts, JWT sessions and a terminal client | `cargo run -p bidirectional-chat-server --locked` |
| [oauth2-demo](oauth2-demo/) | Google sign-in with PKCE, JWT sessions and permissions derived from the identity | `cargo run -p oauth2-demo-server --locked` |
| [file-service-example](file-service-example/) | `file_service!` in one crate: a public download, an authenticated upload and an admin-only route | `cargo run -p file-service-example --locked` |
| [file-service-wasm](file-service-wasm/) | `file_service!` with disk storage, generated OpenAPI and a TypeScript client sample | `cargo run -p file-service-backend --locked` |
| [rest-wasm-example](rest-wasm-example/) | `rest_service!` with path and query parameters, authored OpenAPI examples, the API explorer and a TypeScript client sample | `cargo run -p rest-backend --locked` |
| [wasm-ui-demo](wasm-ui-demo/) | A Dominator browser app that calls `basic-jsonrpc-service` through the generated Rust client | `basic-jsonrpc-service`, then the npm steps in its README |

## How the examples are laid out

Most examples split the service contract from its implementation.
The contract crate (`api/`, `rest-api/` or `file-service-api/`) holds the macro call and the request and response types.
Its `server` and `client` features choose which side the macro generates,
so the server and a client can depend on the same crate.
`file-service-example` keeps everything in one crate to stay short.

The servers that generate OpenAPI or OpenRPC documents at build time
write them under the server crate's own `target/` directory,
for example `examples/rest-wasm-example/rest-backend/target/openapi/userservice.json`.

The credentials in these examples are fixed demo values.
Do not reuse them, or the example secrets, outside local development.

## Tests

The workspace test run includes the examples:

```bash
cargo test --workspace --all-targets --all-features --locked
```

CI also builds `wasm-ui-demo` and runs the browser tests in [tests/playwright](../tests/playwright/).
