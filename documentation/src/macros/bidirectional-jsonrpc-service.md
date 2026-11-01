# `jsonrpc_bidirectional_service!`

`jsonrpc_bidirectional_service!` defines a JSON-RPC API that runs over one WebSocket.
The client calls methods on the server,
and the server pushes notifications to the client and can call methods on it.
From one definition the macro generates the server trait and dispatch,
a typed client, and typed notification handlers.

Use it when the server has to reach the client without being asked:
chat, live dashboards, progress updates.
For plain request and response over HTTP, `jsonrpc_service!` is simpler,
and it can also produce an OpenRPC document, which this macro does not.

## Dependencies and features

Put the macro in a shared API crate,
so the server and its clients compile against the same definition.
The server binary enables the API crate's `server` feature,
and a native, TUI or browser client enables `client`.

```toml
[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["sync"] }
ras-auth-core = "0.3"
ras-jsonrpc-types = "0.3"
ras-jsonrpc-bidirectional-types = "0.3"
ras-jsonrpc-bidirectional-macro = { version = "0.3", default-features = false }
ras-jsonrpc-bidirectional-server = { version = "0.4", optional = true }
ras-jsonrpc-bidirectional-client = { version = "0.4", optional = true }
# Only needed when the service declares `server_to_client_calls`.
uuid = { version = "1", features = ["v4"], optional = true }

[features]
default = []
server = ["dep:ras-jsonrpc-bidirectional-server", "tokio/time", "dep:uuid"]
client = ["dep:ras-jsonrpc-bidirectional-client"]
```

The generated code names these crates by path,
so each of them must be a direct dependency of the API crate.
`tokio` appears because the generated client types use `tokio` channels.
`tokio/time` and `uuid` serve server-to-client calls,
which wait on a timer and need a fresh request id.

Write `feature_gated: true` in the definition, as in the next section.
The generated server code is then wrapped in `#[cfg(feature = "server")]`
and the client code in `#[cfg(feature = "client")]`,
checked against the API crate's own features.
That is why the features above do not need to forward to the macro crate.

Without `feature_gated`, the macro crate's own `server` and `client` features decide
what is generated (both are on by default).
Cargo unifies those features across a build,
so when any crate enables the macro's `server` feature,
an API crate built with only `client` gets the server code as well,
and fails to compile for lack of the server runtime.

The macro crate has one more feature, `permissions`,
which also emits a permission manifest;
see [Permission Manifests](../permission-manifests.md).
It needs `ras-permission-manifest = "0.1"` beside the macro.

## Define the service

```rust,ignore
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub channel: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReceived {
    pub channel: String,
    pub body: String,
}

jsonrpc_bidirectional_service!({
    service_name: ChatService,
    feature_gated: true,
    client_to_server: [
        WITH_PERMISSIONS(["user"]) send_message(SendMessageRequest) -> SendMessageResponse,
    ],
    server_to_client: [
        message_received(MessageReceived),
    ],
    server_to_client_calls: [
    ]
});
```

The fields come in this order.

| Field | Required | Contents |
|---|---|---|
| `service_name: <Ident>` | yes | Prefix for every generated item. |
| `feature_gated: <bool>` | no, default `false` | Gate the generated code on the API crate's `server` and `client` features. Must come straight after `service_name`. |
| `client_to_server: [...]` | yes, may be empty | Methods the client calls: `AUTH name(Request) -> Response`. |
| `server_to_client: [...]` | yes, may be empty | Notifications the server sends: `name(Params)`. No response. |
| `server_to_client_calls: [...]` | yes, may be empty | Methods the server calls on the client: `name(Request) -> Response`. |

Entries in a list are separated by commas, and a trailing comma is allowed.
There is no comma after the last list.
The method name is also the JSON-RPC method name on the wire.

Every `client_to_server` method starts with an auth requirement,
in the same syntax as `jsonrpc_service!`
(see [Auth In The API Contract](../auth-in-api-contract.md)):

- `UNAUTHORIZED` lets anyone call the method.
- `OPTIONAL_AUTH` lets anyone call it, and hands the handler a `ras_auth_core::Caller`
  built from the connection's user, if it has one.
  An anonymous caller only gets this far when the server accepts anonymous connections
  (`require_auth(false)`).
- `WITH_PERMISSIONS(["a", "b"] | ["c"])` requires an authenticated user
  with all permissions of at least one group.
  `WITH_PERMISSIONS([])` accepts any authenticated user.

A `server_to_client_calls` entry needs no auth requirement.
`UNAUTHORIZED` and `WITH_PERMISSIONS(...)` are accepted there,
but nothing checks them, because the call goes out from the server;
they only appear in the permission manifest.
`OPTIONAL_AUTH` there is a compile error.

Payload types must implement `Serialize` and `Deserialize`.
With the client generated, request and response types must also implement `Debug`,
and notification types `Debug` and `Clone`,
because the generated message enums derive those traits.

## What the macro generates

For `service_name: ChatService` the macro generates the following items.

| Item | Side | What it is |
|---|---|---|
| `ChatServiceService` | server | The trait you implement: one method per `client_to_server` entry, one `notify_<name>` per notification, and connection callbacks. |
| `ChatServiceHandler<T, M>` | server | Dispatches requests to your trait implementation; implements `MessageHandler`. |
| `ChatServiceBuilder<T, A>` | server | Builds the WebSocket service from your implementation and an auth provider. |
| `ChatServiceClientHandle<'a>` | server | Typed handle to one connected client, for notifications and server-to-client calls. |
| `ChatServiceClient` | client | Typed methods, `on_<name>` registration, `connect`, `disconnect`, `subscribe`. |
| `ChatServiceClientBuilder` | client | Builds a `ChatServiceClient` from a URL, a token and a request timeout. |
| `ChatServiceClientToServerMessage` | client | Enum with one variant per `client_to_server` method. |
| `ChatServiceServerToClientNotification` | client | Enum with one variant per notification. |

With the `permissions` feature it also emits `generate_chatservice_permission_manifest()`
and a `chatservice_permissions` module of constants.

## Implement the server

Every handler receives the caller's `ConnectionId` and the `ConnectionManager`.
A `WITH_PERMISSIONS` method also receives `&AuthenticatedUser`,
and an `OPTIONAL_AUTH` method receives a `Caller` in the same position.

```rust,ignore
use ras_jsonrpc_bidirectional_types::{ConnectionId, ConnectionManager};

struct ChatServiceImpl;

#[async_trait::async_trait]
impl ChatServiceService for ChatServiceImpl {
    async fn send_message(
        &self,
        client_id: ConnectionId,
        connection_manager: &dyn ConnectionManager,
        _user: &ras_auth_core::AuthenticatedUser,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Echo the message back to the sender as a notification.
        ChatServiceClientHandle::new(client_id, connection_manager)
            .message_received(MessageReceived {
                channel: request.channel,
                body: request.body,
            })
            .await?;
        Ok(SendMessageResponse { message_id: "1".into() })
    }

    async fn notify_message_received(
        &self,
        _connection_id: ConnectionId,
        _params: MessageReceived,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }
}
```

Notice the `notify_message_received` method.
The trait declares one per notification, with no default body,
but the generated code never calls it,
so an empty body is enough.
To send a notification, use `ChatServiceClientHandle`, as above,
or build a `ServerNotification` and pass it to `connection_manager.send_to_connection`
or `broadcast_to_topic`.

The trait also has `on_client_connected`, `on_client_disconnected` and `on_client_authenticated`,
which do nothing unless you override them.

### Mount the server

`ChatServiceBuilder::build()` returns a `BuiltWebSocketService`,
which the server crate's `websocket_handler` serves as an Axum route.
Name the service type in an alias,
because `websocket_handler` needs it spelled out.

```rust,ignore
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager,
    service::{BuiltWebSocketService, websocket_handler},
};

type ChatWebSocket = BuiltWebSocketService<
    ChatServiceHandler<ChatServiceImpl, DefaultConnectionManager>,
    MyAuthProvider,
    DefaultConnectionManager,
>;

let websocket_service = ChatServiceBuilder::new(ChatServiceImpl, MyAuthProvider)
    .require_auth(false)
    .build();

let app = axum::Router::new()
    .route("/ws", axum::routing::get(websocket_handler::<ChatWebSocket>))
    .with_state(websocket_service);
```

`require_auth` is `true` by default,
which refuses the upgrade of any connection without valid credentials.
`require_auth(false)` lets connections without a token in;
`WITH_PERMISSIONS` methods still reject them.
A connection with an invalid token is refused either way.

The generated builder creates its own connection manager.
To share one manager with other parts of the server, for example to broadcast from a background task,
build the handler yourself with `ChatServiceHandler::new(service, manager)`
and `.with_auth_provider(...)`,
and pass it to `WebSocketServiceBuilder` with `build_with_manager`.
[examples/bidirectional-chat](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/bidirectional-chat)
does this.

### Errors the client sees

Handler errors follow the same rule as in
[`jsonrpc_service!`](jsonrpc-service.md#errors-the-client-sees).
An error reaches the client as `-32603 "Internal error"` with no detail,
unless the handler returns a `ras_jsonrpc_types::PublicRpcError` directly
with a code outside the reserved range, or `-32602`:

```rust,ignore
return Err(ras_jsonrpc_types::PublicRpcError::new(1001, "Room is full").into());
```

The `PublicRpcError` must be the returned error itself;
one wrapped inside another error is hidden like any other.
The generated server carries such an error as `ServerError::Rpc`.
A hand-written `MessageRouter` handler can return `ServerError::Rpc` too;
the router applies the same code rule before sending it.

A request without an `id` is a JSON-RPC notification,
and the server never answers it, not even with an error.

This rule covers calls from the client to the server.
In the other direction, a client handler for a server-to-client call returns `Result<T, String>`.
An `Err` reaches the server as code `-32000` with the string as its message,
and the `ChatServiceClientHandle` method returns it as `BidirectionalError::RpcError`.

### Connection security defaults

The defaults are set for production.
Each can be changed on `ras_jsonrpc_bidirectional_server::WebSocketServiceBuilder`,
or by implementing the matching `WebSocketService` method:

| Setting | Default | What it does |
|---|---|---|
| `max_message_size` | 1 MiB | Enforced at the transport, so an oversized frame is rejected before it is buffered. |
| `subscription_limits` | 64 topics per message, 256 per connection, 256-byte names, 100 000 globally | An over-limit `Subscribe` gets an invalid-params error and the connection stays open. |
| `keepalive` | ping every 30 s, close after 90 s idle | Reclaims half-open sockets. Browsers and tungstenite answer pings automatically. |
| `auth_revalidation_interval` | 30 s | Re-runs the auth provider on the connection's token. Failure closes the socket. |
| `on_permission_change` | `DropSubscriptions` | On successful re-validation every held subscription is re-run through `authorize_subscribe`; topics no longer authorized are dropped, and any topic message already queued for the socket is discarded at send time. `Close` closes the socket instead so the client must reconnect. |
| `max_connections` | 10 000 | Enforced atomically with a semaphore permit held for the connection's lifetime. `None` lifts the cap. |

The generated `ChatServiceBuilder` offers all of these except `auth_revalidation_interval`.

Topic subscriptions are default-deny.
A `MessageHandler` allows the topics a connection is entitled to
by overriding `authorize_subscribe`.
The generated handler keeps the default,
so it refuses every `Subscribe` from a client;
to accept them, wrap it in your own `MessageHandler` that delegates to it.
The limits above are enforced inside `ConnectionContext::subscribe`,
so they hold for any handler that subscribes through it.
It returns an error on refusal; the default `handle_subscribe` logs the error and continues.

`WITH_PERMISSIONS` checks go through your provider's `check_permissions`,
so wildcard or hierarchical permission schemes behave the same over WebSocket as over REST.

### Authenticating from a browser

A browser `WebSocket` cannot set headers. The WASM client therefore offers two
subprotocols on upgrade, `ras-jsonrpc` and `token.<jwt>`; the server reads the
token from the second and selects the first, so the token is never echoed in
the response. Native clients send `Authorization: Bearer <token>`. Tokens are
never placed in the URL query string, and the server does not read them from
there.

## Use the client

With the `client` feature, the macro generates a client builder,
one method per `client_to_server` entry, and one `on_<name>` registration method
per notification and per server-to-client call:

```rust,ignore
let mut client = ChatServiceClientBuilder::new("ws://localhost:3000/ws")
    .with_jwt_token(token)
    .build()
    .await?;

client.on_message_received(|message| {
    println!("{}: {}", message.channel, message.body);
});

client.connect().await?;

let sent = client
    .send_message(SendMessageRequest {
        channel: "general".to_string(),
        body: "hello".to_string(),
    })
    .await?;
```

Register handlers before `connect`, so no notification arrives before its handler.
A handler for a server-to-client call is a closure
that returns a future of `Result<Response, String>`.

A call that fails with a JSON-RPC error returns `ClientError::Rpc(JsonRpcError)`,
with the code, message and data the server sent.

`ChatServiceClientBuilder` only sets the URL, the token and the request timeout.
For headers, timeouts or reconnect settings, build a `ras_jsonrpc_bidirectional_client::Client`
with its own `ClientBuilder`, and wrap it with `ChatServiceClient::new(client)`.

See
[examples/bidirectional-chat](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/bidirectional-chat)
for a complete server and terminal client.
