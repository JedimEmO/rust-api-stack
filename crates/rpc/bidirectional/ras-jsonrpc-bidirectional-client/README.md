# ras-jsonrpc-bidirectional-client

A WebSocket client for bidirectional JSON-RPC, for native targets and for the browser.
It calls methods on the server, receives the server's notifications,
and answers calls the server makes to it.

The server side is
[`ras-jsonrpc-bidirectional-server`](../ras-jsonrpc-bidirectional-server).
When the API is defined with the
[`jsonrpc_bidirectional_service!`](../ras-jsonrpc-bidirectional-macro) macro,
the generated `<Name>Client` wraps this client and gives each method a typed signature;
see the [guide](../../../../documentation/src/macros/bidirectional-jsonrpc-service.md).
The generated code reaches `serde`, `serde_json` and `tokio` through this crate,
so the API crate does not depend on them for the client side.
A notification whose params do not decode into the declared type is logged as a warning and dropped.
This README covers the untyped client underneath.

## Installing

On native targets the default `native` feature uses `tokio-tungstenite`:

```toml
[dependencies]
ras-jsonrpc-bidirectional-client = "0.4"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

In the browser the `wasm` feature uses the `web-sys` WebSocket:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
ras-jsonrpc-bidirectional-client = { version = "0.4", default-features = false, features = ["wasm"] }
```

## Calling the server

```rust
use ras_jsonrpc_bidirectional_client::ClientBuilder;
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ClientBuilder::new("ws://localhost:8080/ws")
        .with_jwt_token("demo-token".to_string())
        .build()
        .await?;

    client.on_notification(
        "user_message",
        Arc::new(|method, params| println!("{method}: {params}")),
    );

    client.connect().await?;

    let response = client.call("get_user", Some(json!({ "user_id": 123 }))).await?;
    match (response.result, response.error) {
        (Some(result), _) => println!("user: {result}"),
        (_, Some(error)) => println!("error {}: {}", error.code, error.message),
        _ => {}
    }

    client.notify("user_activity", Some(json!({ "action": "page_view" }))).await?;
    Ok(())
}
```

Notice that `call` returns the raw `JsonRpcResponse`,
so a JSON-RPC error from the server arrives as `Ok` with `error` set.
The generated typed client turns it into `ClientError::Rpc`.
`notify` sends a request without an `id`, which the server never answers.

`call`, `notify` and `subscribe` fail with `ClientError::NotConnected` until `connect` has succeeded.
`.with_auto_connect(true)` on the builder connects inside `build`.

Other handlers are registered the same way:
`on_rpc_request` answers a method the server calls on the client,
`on_connection_event` receives `ConnectionEvent::Connected` and `Disconnected`,
and `subscribe(topic, handler)` asks the server for broadcasts on a topic.
The server refuses topics it has not authorized.

## Configuration

`ClientBuilder` sets the request timeout (30 s by default), the connection timeout (10 s),
the heartbeat interval (30 s, `None` turns it off) and extra headers:

```rust
use std::time::Duration;

let client = ClientBuilder::new("wss://api.example.com/ws")
    .with_jwt_token(token)
    .with_header("User-Agent", "MyApp/1.0")
    .with_request_timeout(Duration::from_secs(10))
    .with_connection_timeout(Duration::from_secs(5))
    .with_heartbeat_interval(Some(Duration::from_secs(30)))
    .build()
    .await?;
```

A browser cannot set headers on a WebSocket,
so `with_header` has no effect with the `wasm` feature.

Neither transport accepts a message over 16 MiB.
In the browser, messages wait in a queue until the client handles them,
because the WebSocket API cannot push back on the server.
The queue holds at most `message_buffer_size` messages (1024 by default) and 64 MiB;
a server that sends past that is disconnected, so it cannot fill the page's memory.

`ClientConfig`'s `Debug` output shows the token and header values as `<redacted>`,
so a logged configuration does not leak credentials.

## Authentication

On native targets the token is sent as `Authorization: Bearer <token>`.
In the browser the client offers the subprotocols `ras-jsonrpc` and `token.<jwt>`;
the server reads the token from the second and selects the first,
so the token is never echoed back.
Tokens are never placed in the URL,
because query strings end up in proxy logs, browser history and tracing spans,
and the server does not read them there.

## Errors and reconnecting

Every fallible method returns `ClientResult<T>`, an alias for `Result<T, ClientError>`.
`ClientError` is `#[non_exhaustive]`, so a `match` on it needs a wildcard arm.
Its variants include `NotConnected`, `Timeout { timeout_seconds }`, `Authentication`, `Rpc`
and, on `wasm32`, `JavaScript`.

The client does not reconnect on its own.
When a call returns `ClientError::NotConnected`, or `client.state().await` is `ClientState::Failed`,
call `connect` again from your own retry loop.
`ReconnectConfig` computes backoff delays with jitter and caps the number of attempts,
for loops that want a shared policy.

## Example

[`examples/bidirectional_client_usage.rs`](examples/bidirectional_client_usage.rs)
connects, registers notification and connection-event handlers, makes calls and subscribes to a topic.
Point it at a server with `BIDIRECTIONAL_CLIENT_URL` and `BIDIRECTIONAL_CLIENT_TOKEN`.

## License

Licensed under either MIT or Apache-2.0, at your option.
See [LICENSE-MIT](../../../../LICENSE-MIT) and [LICENSE-APACHE](../../../../LICENSE-APACHE).
