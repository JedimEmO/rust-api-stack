//! A bad credential on a service that admits anonymous connections connects
//! anonymously, as public REST, file and JSON-RPC routes ignore it; a service
//! that requires auth still refuses the upgrade.

use std::{sync::Arc, time::Duration};

use axum::Router;
use futures::{SinkExt, StreamExt};
use ras_auth_core::{AuthFuture, AuthProvider};
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager, MessageRouter, WebSocketServiceBuilder,
    service::{BuiltWebSocketService, websocket_handler},
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

struct DenyAll;

impl AuthProvider for DenyAll {
    fn authenticate(&self, _token: String) -> AuthFuture<'_> {
        Box::pin(async { Err(ras_auth_core::AuthError::InvalidToken) })
    }
}

async fn spawn_server(require_auth: bool) -> String {
    let service = WebSocketServiceBuilder::builder()
        .handler(Arc::new(MessageRouter::new()))
        .auth_provider(Arc::new(DenyAll))
        .require_auth(require_auth)
        .auth_revalidation_interval(Duration::from_millis(50))
        .build()
        .build();
    type Svc = BuiltWebSocketService<MessageRouter, DenyAll, DefaultConnectionManager>;
    let app = Router::new()
        .route("/ws", axum::routing::get(websocket_handler::<Svc>))
        .with_state(service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("ws://{addr}/ws")
}

fn request_with_bad_token(url: &str) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", "Bearer not-a-real-token".parse().unwrap());
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_token_connects_anonymously_when_auth_is_optional() {
    let url = spawn_server(false).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(request_with_bad_token(&url))
        .await
        .expect("the upgrade succeeds without a valid credential");

    // Outlive several re-validation intervals: an anonymous connection holds
    // no credential to re-validate, so it must not be closed for the bad one.
    tokio::time::sleep(Duration::from_millis(250)).await;

    socket
        .send(Message::text(
            r#"{"jsonrpc":"2.0","method":"missing","params":null,"id":1}"#,
        ))
        .await
        .expect("the connection is still open");
    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                // Skip the `connection_established` greeting.
                Some(Ok(Message::Text(text))) => {
                    let message: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if message["id"] == 1 {
                        break message;
                    }
                }
                Some(Ok(_)) => continue,
                other => panic!("connection ended: {other:?}"),
            }
        }
    })
    .await
    .expect("a reply arrives");
    assert_eq!(reply["error"]["code"], -32601, "{reply}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_token_is_refused_when_auth_is_required() {
    let url = spawn_server(true).await;
    let error = tokio_tungstenite::connect_async(request_with_bad_token(&url))
        .await
        .expect_err("the upgrade is refused");
    match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), 401);
        }
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }
}
