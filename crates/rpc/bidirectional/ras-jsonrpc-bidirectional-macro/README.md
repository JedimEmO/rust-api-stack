# ras-jsonrpc-bidirectional-macro

The `jsonrpc_bidirectional_service!` macro defines a JSON-RPC API that runs over one WebSocket,
with calls in both directions and notifications from the server.

A request-and-response API cannot tell the client that something happened;
the client has to ask again.
Over a WebSocket the server can push,
but then both ends need to agree on every message by hand.
This macro takes one definition and generates the server trait, its dispatch and auth checks,
a typed client, and typed notification handlers,
so the server and the client cannot drift apart.

The full guide is
[`jsonrpc_bidirectional_service!`](../../../../documentation/src/macros/bidirectional-jsonrpc-service.md)
in the mdBook.
It covers the dependencies, every field of the definition, the generated items,
errors, connection limits and the client.

## What it looks like

```rust,ignore
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;

jsonrpc_bidirectional_service!({
    service_name: ChatService,
    feature_gated: true,
    client_to_server: [
        UNAUTHORIZED list_rooms(()) -> Vec<String>,
        OPTIONAL_AUTH join_room(JoinRoom) -> RoomState,
        WITH_PERMISSIONS(["user"]) send_message(SendMessage) -> MessageId,
        WITH_PERMISSIONS(["moderator"] | ["admin"]) kick_user(KickUser) -> bool,
    ],
    server_to_client: [
        message_received(MessageReceived),
    ],
    server_to_client_calls: [
        confirm_leave(ConfirmLeave) -> bool,
    ]
});
```

`client_to_server` lists the methods the client calls, each with an auth requirement.
`server_to_client` lists notifications, which get no response.
`server_to_client_calls` lists methods the server calls on the client and waits for.
They take no auth requirement, because the server is the caller.
All three lists are required, in this order, and may be empty.

On the server side this generates the `ChatServiceService` trait to implement,
with one method per client call and a default-deny `authorize_subscribe` for topics,
`ChatServiceBuilder` to turn it into an Axum-ready WebSocket service,
and `ChatServiceClientHandle` to notify or call one connected client.
On the client side it generates `ChatServiceClient`,
with `list_rooms`, `send_message` and the other methods,
and `on_message_received` and `on_confirm_leave` to register handlers.

## Installing

The macro belongs in a shared API crate that the server and the clients both depend on.

```toml
[dependencies]
ras-jsonrpc-bidirectional-macro = { version = "0.3", default-features = false }
```

The generated server code reaches everything it uses through `ras-jsonrpc-bidirectional-server`,
and the client code through `ras-jsonrpc-bidirectional-client`.
So the API crate needs those two, behind its `server` and `client` features,
and `serde` for its own payload types.
The guide has the complete `Cargo.toml`.

## Cargo features

| Feature | Default | Effect |
|---|---|---|
| `server` | on | Generate the server items. |
| `client` | on | Generate the client items. |
| `permissions` | off | Also generate a permission manifest; needs `ras-permission-manifest`. |

With `feature_gated: true` in the definition,
the server and client items are always generated,
behind `#[cfg(feature = "server")]` and `#[cfg(feature = "client")]` on the API crate's own features,
and the macro's `server` and `client` features have no effect.
That is the recommended setup,
because Cargo unifies the macro's features across a workspace.

## Related crates

- [`ras-jsonrpc-bidirectional-server`](../ras-jsonrpc-bidirectional-server): the Axum WebSocket server the generated code runs on.
- [`ras-jsonrpc-bidirectional-client`](../ras-jsonrpc-bidirectional-client): the native and browser client the generated client wraps.
- [`ras-jsonrpc-bidirectional-types`](../ras-jsonrpc-bidirectional-types): message and connection types shared by both.
- [`examples/bidirectional-chat`](../../../../examples/bidirectional-chat): a chat server and terminal client built on this macro.

This macro does not generate an OpenRPC document.
For that, use `jsonrpc_service!` from `ras-jsonrpc-macro`, which serves JSON-RPC over HTTP.

## License

Licensed under either MIT or Apache-2.0, at your option.
See [LICENSE-MIT](../../../../LICENSE-MIT) and [LICENSE-APACHE](../../../../LICENSE-APACHE).
