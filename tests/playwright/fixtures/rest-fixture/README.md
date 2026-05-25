# Playwright REST Fixture

Socket-bound fixture server for the REST API explorer browser tests. It is intentionally small and deterministic so Playwright can exercise the generated explorer with a real browser-visible HTTP server.

## Routes

The service is mounted at `/api/v1` and exposes:

- Explorer page: `/api/v1/docs`
- OpenAPI document: `/api/v1/docs/openapi.json`
- Health route: `/api/v1/health`
- Public widget routes
- Permission-gated widget/profile routes
- Versioned rename routes used by compatibility tests

The contract in [src/main.rs](src/main.rs) includes Markdown operation docs, schema field docs, auth-protected operations, query parameters, path parameters, and versioned route metadata.

## Run

From the workspace root:

```bash
PLAYWRIGHT_REST_ADDR=127.0.0.1:3101 cargo run --locked -p playwright-rest-fixture
```

The Playwright config starts this server automatically. Use `PLAYWRIGHT_REST_PORT` when running the full browser suite to avoid local port collisions.

## Test Tokens

- `user-token`
- `admin-token`

## Checks

```bash
cargo check -p playwright-rest-fixture --locked
cargo test -p playwright-rest-fixture --locked
cargo clippy -p playwright-rest-fixture --all-targets --all-features --locked -- -D warnings
```

See [../../README.md](../../README.md) for the full Playwright suite.
