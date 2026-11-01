# ras-jsonrpc-bidirectional-server

The Axum WebSocket server for bidirectional JSON-RPC:
the client calls methods on the server,
and the server sends notifications and calls back over the same socket.

It authenticates the connection during the upgrade,
dispatches JSON-RPC requests to a handler,
tracks connections and topic subscriptions,
and enforces limits on message size, connections and subscriptions.

Most services do not use this crate directly.
The [`jsonrpc_bidirectional_service!`](../ras-jsonrpc-bidirectional-macro) macro generates
a handler and a builder on top of it;
the [guide](../../../../documentation/src/macros/bidirectional-jsonrpc-service.md)
shows how to define, mount and configure such a service.
This crate re-exports `async_trait`, `ConnectionId`, `ConnectionManager`, `AuthenticatedUser`
and `Caller`, the names a generated `<Name>Service` implementation needs,
so a server depends on this crate and the API crate and nothing else for the WebSocket side.
This README covers the parts you use by hand.

## A server without the macro

`MessageRouter` maps JSON-RPC method names to closures.
It suits small services and tests, where a typed contract is not worth the setup.

```rust
use axum::{Router, routing::get};
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager, MessageRouter, ServerError,
    service::{BuiltWebSocketService, create_router_service, websocket_handler},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token != "demo-token" {
                return Err(AuthError::InvalidToken);
            }
            Ok(AuthenticatedUser {
                user_id: "demo-user".to_string(),
                permissions: HashSet::from(["user".to_string()]),
                metadata: None,
            })
        })
    }
}

type EchoService =
    BuiltWebSocketService<MessageRouter, DemoAuthProvider, DefaultConnectionManager>;

#[tokio::main]
async fn main() {
    let mut router = MessageRouter::new();
    router.register_value("echo", |request, _context| async move {
        Ok::<Value, ServerError>(request.params.unwrap_or(json!(null)))
    });

    // `true` refuses connections that present no token.
    let service = create_router_service(router, Arc::new(DemoAuthProvider), true);

    let app = Router::new()
        .route("/ws", get(websocket_handler::<EchoService>))
        .with_state(service);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

`register_value` wraps the returned value in a JSON-RPC response.
With `register` the closure builds the response itself,
and with `register_notification` it never answers.
A request without an `id` is a JSON-RPC notification and gets no response,
whichever registration handled it.

A handler error reaches the client as `-32603 "Internal error"` with no detail.
The exception is `ServerError::Rpc` with a code the handler may use:
any code outside the reserved range `-32768..=-32000`, or `-32602`.
Return `ServerError::InvalidParams` when the params do not decode;
the client gets `-32602 "Invalid params"`, which is also what the generated handler sends.
`ServerError` is `#[non_exhaustive]`.

For full control, implement `MessageHandler` yourself.
Its `authorize_subscribe` decides which topics a connection may subscribe to,
and it refuses every topic unless you override it.
The handler the macro generates passes the question on to the service's own `authorize_subscribe`.

## Configuring the service

`create_router_service` uses the defaults.
For anything else, use `WebSocketServiceBuilder`.
It is a two-step builder: the first `build()` finishes the settings,
and the second turns them into the service.

```rust
use ras_jsonrpc_bidirectional_server::{KeepaliveConfig, WebSocketServiceBuilder};
use std::{sync::Arc, time::Duration};

let service = WebSocketServiceBuilder::builder()
    .handler(Arc::new(router))
    .auth_provider(Arc::new(DemoAuthProvider))
    .require_auth(true)
    .max_message_size(256 * 1024)
    .keepalive(KeepaliveConfig {
        ping_interval: Some(Duration::from_secs(20)),
        idle_timeout: Some(Duration::from_secs(60)),
    })
    .build()
    .build();
```

To share the connection manager with the rest of the application,
for example to broadcast from a background task,
create it first and finish with `.build_with_manager(manager)` instead of the second `build()`.

The defaults for message size, subscriptions, keepalive, credential re-validation
and connection count are listed in the guide under
[Connection security defaults](../../../../documentation/src/macros/bidirectional-jsonrpc-service.md#connection-security-defaults).

## Authentication

The server authenticates during the WebSocket upgrade.
It takes the token from the first of these that is present:

1. `Authorization: Bearer <token>`. A malformed `Authorization` header is not a token,
   and the server does not fall back to the other sources.
2. `Sec-WebSocket-Protocol: ras-jsonrpc, token.<jwt>`, for browsers,
   which cannot set headers on a WebSocket.
   The server selects `ras-jsonrpc`, so the token is not echoed back.
3. `X-Auth-Token: <token>`.

With `require_auth(true)`, the default, a missing token or one the auth provider rejects refuses the upgrade.
With `require_auth(false)`, both connect anonymously.
Tokens in the URL query string are never read.

While the connection is open,
the server re-runs the auth provider on the same token every 30 seconds by default,
and closes the socket when it fails.
An anonymous connection has no token to re-run.

## Sending to clients

`DefaultConnectionManager` implements the `ConnectionManager` trait
from `ras-jsonrpc-bidirectional-types`.
Handlers receive it, and the server sends through it:

```rust
use ras_jsonrpc_bidirectional_server::ConnectionManager;

manager.send_to_connection(connection_id, message).await?;
manager.broadcast_to_topic("updates", message).await?;
manager.broadcast_to_authenticated(message).await?;
manager.broadcast_to_permission("admin", message).await?;
```

Each `message` is a `BidirectionalMessage`,
usually a `ServerNotification` or a `Broadcast`.
The broadcast methods return how many connections the message went to.

## Related crates

- [`ras-jsonrpc-bidirectional-macro`](../ras-jsonrpc-bidirectional-macro): generates typed services on top of this server.
- [`ras-jsonrpc-bidirectional-client`](../ras-jsonrpc-bidirectional-client): the matching native and browser client.
- [`ras-jsonrpc-bidirectional-types`](../ras-jsonrpc-bidirectional-types): message, connection and manager types.

## License

Licensed under either MIT or Apache-2.0, at your option.
See [LICENSE-MIT](../../../../LICENSE-MIT) and [LICENSE-APACHE](../../../../LICENSE-APACHE).
