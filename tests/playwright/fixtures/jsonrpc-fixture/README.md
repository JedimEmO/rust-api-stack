# Playwright JSON-RPC Fixture

Socket-bound fixture server for the JSON-RPC API explorer browser tests. It is not a reusable example server; it exists so Playwright can load a real explorer page and exercise browser behavior.

## Routes

The service is mounted at `/rpc` and exposes:

- JSON-RPC endpoint: `/rpc`
- Explorer page: `/rpc/explorer`
- OpenRPC document: `/rpc/explorer/openrpc.json`

The contract in [src/main.rs](src/main.rs) includes public methods, permission-gated methods, Markdown doc comments, schema field docs, and versioned compatibility methods. The browser tests use those cases to verify explorer rendering and request behavior.

## Run

From the workspace root:

```bash
PLAYWRIGHT_JSONRPC_ADDR=127.0.0.1:3102 cargo run --locked -p playwright-jsonrpc-fixture
```

The Playwright config starts this server automatically. Use `PLAYWRIGHT_JSONRPC_PORT` when running the full browser suite to avoid local port collisions.

## Test Tokens

- `user-token`
- `admin-token`

## Checks

```bash
cargo check -p playwright-jsonrpc-fixture --locked
cargo test -p playwright-jsonrpc-fixture --locked
cargo clippy -p playwright-jsonrpc-fixture --all-targets --all-features --locked -- -D warnings
```

See [../../README.md](../../README.md) for the full Playwright suite.
