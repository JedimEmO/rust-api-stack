# WASM UI Demo

Browser-based task management UI built with Dominator, dwind styling, and the generated JSON-RPC client from `basic-jsonrpc-api`.

## What It Shows

- Rust UI compiled to WebAssembly with Rollup.
- Generated JSON-RPC client calls from the browser.
- Login, task list, task creation, profile, and dashboard flows.
- Reactive state with `futures-signals`.
- Local serving with `/rpc` proxied to `basic-jsonrpc-service`.

## Run It

Start the JSON-RPC backend:

```bash
cargo run -p basic-jsonrpc-service --locked
```

In another terminal, build and serve the UI:

```bash
npm --prefix examples/wasm-ui-demo ci
npm --prefix examples/wasm-ui-demo start
```

Requires Node.js 22.13 or newer.

Open `http://localhost:8080`. The local Vite server proxies `/rpc` to `http://localhost:3000/rpc`.

## Credentials

- User: `user` / `password`
- Admin: `admin` / `secret`

## Build

```bash
npm --prefix examples/wasm-ui-demo run build
```

The browser bundle is written to `dist/`.

The Rollup Rust plugin uses `wasm-opt` when it is available. If `wasm-opt` is not installed, the build still succeeds and skips that optimization step.

## Checks

```bash
cargo test -p wasm-ui-demo --locked
cargo clippy -p wasm-ui-demo --all-targets --all-features --locked -- -D warnings
cargo check -p wasm-ui-demo --target wasm32-unknown-unknown --locked
```

## Notes

This demo expects the backend JSON-RPC route at `/rpc`. If you serve the built `dist/` directory behind another host, proxy `/rpc` to the basic JSON-RPC service or serve both from the same origin.
