# API explorer Playwright tests

Browser tests for the API explorers that `rest_service!` and `jsonrpc_service!` generate.
Playwright starts two Rust fixture servers, [rest-fixture](fixtures/rest-fixture/README.md)
and [jsonrpc-fixture](fixtures/jsonrpc-fixture/README.md),
and drives their explorers in Chromium.
The fixtures declare versioned endpoints with a current and an older version,
so the tests cover both.

## Local setup

```bash
npm --prefix tests/playwright ci
npm --prefix tests/playwright run install:browsers
npm --prefix tests/playwright test
```

For headed debugging:

```bash
npm --prefix tests/playwright run test:headed
```

The fixtures use:

- REST: `http://127.0.0.1:3101/api/v1/docs`
- JSON-RPC: `http://127.0.0.1:3102/rpc/explorer`

To avoid local port collisions, override the ports before running the suite:

```bash
PLAYWRIGHT_REST_PORT=3201 PLAYWRIGHT_JSONRPC_PORT=3202 npm --prefix tests/playwright test
```

Test tokens:

- `user-token`
- `admin-token`

## WASM UI interactions

Build the UI and run its separate Chromium suite:

```bash
npm --prefix examples/wasm-ui-demo ci
npm --prefix examples/wasm-ui-demo run build
npm --prefix tests/playwright test -- --config wasm-ui.config.ts
```

Run these commands from the repository root after installing the Playwright browser.
The suite serves the compiled bundle and supplies deterministic JSON-RPC responses.
It checks login failure/success, task list/create/complete/delete, failed-create state,
and browser panics. CI runs it in the WASM UI example job after building the bundle.
