# Introduction

Rust Agent Stack (RAS) is a set of Rust crates for building typed,
authenticated HTTP and WebSocket APIs on Axum.
You declare an API once, in Rust,
and a macro generates the server boundary, the handler trait, the clients,
the API documents and the auth checks from that one declaration.

There is one macro per kind of API:

- `jsonrpc_service!` for JSON-RPC over HTTP.
- `rest_service!` for conventional JSON REST APIs.
- `file_service!` for streaming uploads and downloads.
- `jsonrpc_bidirectional_service!` for typed JSON-RPC in both directions over a WebSocket.

They share one workflow.
Define the wire contract, implement the generated trait,
configure an auth provider when protected operations exist,
then mount the generated router or call the generated client.

The [application tutorial](tutorial/index.md) walks through that workflow for a small app,
from crate boundaries and auth to the server, the clients, tests and API evolution.
The macro guides are the reference for each macro's syntax and options.

The repository has runnable
[examples](https://github.com/JedimEmO/rust-api-stack/tree/master/examples)
for JSON-RPC, REST, file services, OAuth2, a bidirectional chat,
and browser clients in TypeScript and Rust/WASM.
