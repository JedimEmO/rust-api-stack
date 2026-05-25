# `jsonrpc_bidirectional_service!`

Use `jsonrpc_bidirectional_service!` for typed JSON-RPC traffic over
WebSockets. It generates server-side dispatch for client calls, client-side
method helpers, typed notification handling, and optional server-to-client
request support.

## Dependencies And Features

```toml
[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ras-auth-core = "0.1.0"
ras-jsonrpc-types = "0.1.1"
ras-jsonrpc-bidirectional-types = "0.1.0"
ras-jsonrpc-bidirectional-macro = { version = "0.1.0", default-features = false }
ras-jsonrpc-bidirectional-server = { version = "0.1.0", optional = true }
ras-jsonrpc-bidirectional-client = { version = "0.1.0", optional = true }

[features]
default = []
server = [
    "ras-jsonrpc-bidirectional-macro/server",
    "dep:ras-jsonrpc-bidirectional-server",
]
client = [
    "ras-jsonrpc-bidirectional-macro/client",
    "dep:ras-jsonrpc-bidirectional-client",
]
```

These API-crate features forward to the macro crate and enable the runtime
dependencies used by the generated surface. The WebSocket server depends on the
API crate with `features = ["server"]`; TUI, native, or browser clients depend
on it with `features = ["client"]`.

If `server_to_client_calls` is used, the server feature also needs optional
`tokio` and `uuid` dependencies because generated server-side client handles
track pending responses and timeouts.

## Define The Service

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

`client_to_server` methods support the same `UNAUTHORIZED` and
`WITH_PERMISSIONS(["a"] | ["b", "c"])` style as the HTTP JSON-RPC macro.

## Implement And Mount The Server

Server handlers receive the connection id and connection manager. Protected
methods also receive `&AuthenticatedUser`.

```rust,ignore
#[async_trait::async_trait]
impl ChatServiceService for ChatServiceImpl {
    async fn send_message(
        &self,
        client_id: ras_jsonrpc_bidirectional_types::ConnectionId,
        connection_manager: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager,
        user: &ras_auth_core::AuthenticatedUser,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!("persist and broadcast the message")
    }

    async fn notify_message_received(
        &self,
        connection_id: ras_jsonrpc_bidirectional_types::ConnectionId,
        params: MessageReceived,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }
}
```

```rust,ignore
let websocket_service = ChatServiceBuilder::new(ChatServiceImpl, my_auth_provider)
    .require_auth(false)
    .build();

let app = axum::Router::new()
    .route("/ws", axum::routing::get(ras_jsonrpc_bidirectional_server::websocket_handler::<_>))
    .with_state(websocket_service);
```

`require_auth(true)` requires credentials for the connection as a whole.
Method-level permissions are still enforced for protected calls.

## Client Usage

The client feature generates a typed client builder, method calls, connection
helpers, and notification registration:

```rust,ignore
let mut client = ChatServiceClientBuilder::new("ws://localhost:3000/ws")
    .with_jwt_token(token)
    .build()
    .await?;

client.on_message_received(|message| {
    println!("{}: {}", message.channel, message.body);
});

client.connect().await?;
let sent = client.send_message(SendMessageRequest {
    channel: "general".to_string(),
    body: "hello".to_string(),
}).await?;
```

In application code, it is usually useful to register all notification handlers
before `connect`, then wrap common calls behind a small app-level client:

```rust,ignore
client.on_message_received(|message| {
    println!("{}: {}", message.channel, message.body);
});

client.on_user_joined(|event| {
    println!("{} joined", event.username);
});

client.connect().await?;

let rooms = client.list_rooms(ListRoomsRequest {}).await?;

client
    .send_message(SendMessageRequest {
        channel: rooms.default_channel,
        body: "hello".to_string(),
    })
    .await?;
```

This macro does not currently generate OpenRPC. Use HTTP
`jsonrpc_service!` when an OpenRPC document is required.

See
[examples/bidirectional-chat](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/bidirectional-chat).
