# Introduction

Rust Agent Stack (RAS) is a set of Rust crates for building type-safe,
authenticated service APIs. The central idea is that the API contract should be
declared once, in Rust, and then used to generate the server boundary, handler
trait, clients, API documents, and authentication checks.

The main service macros are:

- `jsonrpc_service!` for HTTP JSON-RPC services.
- `rest_service!` for conventional JSON REST APIs.
- `file_service!` for streaming upload and download APIs.
- `jsonrpc_bidirectional_service!` for typed bidirectional JSON-RPC over
  WebSockets.

Each macro follows the same shape: define the wire contract, implement the
generated trait, configure an auth provider when protected calls exist, then
mount the generated server or use the generated client.

If you want a guided application design flow, start with the
[application tutorial](tutorial/index.md). It walks through crate boundaries,
auth, server implementation, generated clients, tests, and evolution.

The repository contains runnable
[examples](https://github.com/JedimEmO/rust-api-stack/tree/master/examples),
including JSON-RPC, REST, file services, OAuth2, bidirectional chat, and
browser/WASM clients.
