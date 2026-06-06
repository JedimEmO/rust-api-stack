# ras-transport-core

HTTP transport abstraction for generated Rust Agent Stack clients.

Generated REST / JSON-RPC / File clients dispatch through the `HttpTransport`
trait instead of hard-coding `reqwest::Client`. Two implementations ship here:

- `ReqwestTransport` (production, `reqwest` feature) — a dumb pipe over
  `reqwest::Client` that streams request and response bodies on native targets.
- `AxumTestTransport` (`axum-test` feature, native only) — wraps an
  `axum_test::TestServer` so generated clients can be exercised end-to-end
  against a server with no sockets.

This trait is the HTTP sibling of the `WebSocketTransport` abstraction in
`ras-jsonrpc-bidirectional-client`, following the same dyn-dispatch +
conditional-`Send` (`async_trait(?Send)` on wasm + `TransportThreadBounds`)
pattern so a single `Arc<dyn _>` works on both native and wasm.

## Features

- `reqwest` — production transport (declared for both native and wasm targets).
- `fs` — native file-part streaming from disk (`MultipartBuilder::file_path` /
  `stream_part`), pulls in `tokio` + `tokio-util`.
- `axum-test` — in-process test transport (native only).

## WASM

WASM is a hard target. The fetch API cannot stream request bodies, so on wasm
`RequestBody::Stream` is collected before sending and `MultipartBuilder`
file/stream parts are `fs`-gated (native only). Response bodies still work.
