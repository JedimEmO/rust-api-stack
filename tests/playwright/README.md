# API Explorer Playwright Tests

These tests exercise the generated REST and JSON-RPC API explorers in a real
browser. Dedicated Rust fixture servers are started by Playwright. The fixtures
include canonical and legacy versioned API entries so the explorer covers both
current and compatibility routes.

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
and browser panics. Run it after building the WASM UI example.
