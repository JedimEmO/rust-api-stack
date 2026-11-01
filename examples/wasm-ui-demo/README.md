# WASM UI demo

A browser task manager written in Rust with Dominator and dwind,
compiled to WebAssembly, that talks to the [basic JSON-RPC service](../basic-jsonrpc/service/README.md)
through the `MyServiceClient` generated in `basic-jsonrpc-api`.
The browser and the server share the request and response types, so there is no hand-written API layer.

It covers signing in, listing, creating, completing and deleting tasks,
and the dashboard statistics, with state held in `futures-signals` signals.

## Run

You need Node.js 22.13 or newer and the `wasm32-unknown-unknown` Rust target
(`rustup target add wasm32-unknown-unknown`).

Start the backend from the repository root:

```bash
cargo run -p basic-jsonrpc-service --locked
```

In another terminal, build the app and serve it:

```bash
npm --prefix examples/wasm-ui-demo ci
npm --prefix examples/wasm-ui-demo start
```

Open `http://localhost:8080` and sign in as `user` / `password` or `admin` / `secret`.

The app sends JSON-RPC requests to `/rpc` on the page's own origin.
[vite.config.js](vite.config.js) proxies `/rpc` to `http://localhost:3000`.
The `start` script passes `--config` explicitly,
because Vite otherwise looks for its configuration in the served directory, `dist`, and starts without the proxy.

## Build

```bash
npm --prefix examples/wasm-ui-demo run build
```

Rollup compiles the crate with `@wasm-tool/rollup-plugin-rust` and writes the bundle to `dist/`.
The plugin runs `wasm-opt` when it is installed and skips that step otherwise.

To serve `dist/` from somewhere else, proxy `/rpc` to the JSON-RPC service or serve both from one origin.

## Checks

```bash
cargo test -p wasm-ui-demo --locked
cargo clippy -p wasm-ui-demo --all-targets --all-features --locked -- -D warnings
cargo check -p wasm-ui-demo --target wasm32-unknown-unknown --locked
```

The Playwright suite in [tests/playwright](../../tests/playwright/README.md#wasm-ui-interactions)
drives the built app in Chromium against stubbed JSON-RPC responses.
