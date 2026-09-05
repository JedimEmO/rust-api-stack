//! W2: the inbound message limit is enforced by the WebSocket transport, so an
//! oversized frame is rejected before it is buffered rather than after.

use std::sync::Arc;

use axum::Router;
use futures::{SinkExt, StreamExt};
use ras_auth_core::{AuthFuture, AuthProvider};
use ras_jsonrpc_bidirectional_server::{
    DefaultConnectionManager, MessageRouter, WebSocketServiceBuilder,
    service::{BuiltWebSocketService, websocket_handler},
};
use tokio_tungstenite::tungstenite::Message;

struct DenyAll;

impl AuthProvider for DenyAll {
    fn authenticate(&self, _token: String) -> AuthFuture<'_> {
        Box::pin(async { Err(ras_auth_core::AuthError::InvalidToken) })
    }
}

const LIMIT: usize = 1024;

async fn spawn_server() -> String {
    let service = WebSocketServiceBuilder::builder()
        .handler(Arc::new(MessageRouter::new()))
        .auth_provider(Arc::new(DenyAll))
        .require_auth(false)
        .max_message_size(LIMIT)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w2_oversized_frame_is_rejected_at_the_transport() {
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let url = spawn_server().await;
    let addr = url.trim_start_matches("ws://").trim_end_matches("/ws");

    // Raw handshake so we control the frame bytes.
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    tcp.write_all(
        format!(
            "GET /ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        tcp.read_exact(&mut byte).await.unwrap();
        buf.push(byte[0]);
    }
    assert!(
        buf.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&buf)
    );

    // A masked text frame header announcing a payload four times the limit,
    // followed by only a few payload bytes. A transport-level limit rejects
    // the header outright; a post-buffer check would sit waiting for the rest
    // of the payload that never arrives.
    let announced = (LIMIT * 4) as u16;
    let mut frame = vec![0x81, 0x80 | 126];
    frame.extend_from_slice(&announced.to_be_bytes());
    frame.extend_from_slice(&[0, 0, 0, 0]); // mask key
    frame.extend_from_slice(b"partial");
    tcp.write_all(&frame).await.unwrap();

    // Drain until EOF or close frame; must happen well before the idle timeout.
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        let mut sink = [0u8; 4096];
        loop {
            match tcp.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) if sink[..n].contains(&0x88) => break, // close opcode
                Ok(_) => {}
            }
        }
    })
    .await;
    assert!(
        outcome.is_ok(),
        "server must reject an oversized frame header at the transport instead of buffering"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w2_frame_under_the_limit_still_reaches_the_jsonrpc_layer() {
    let url = spawn_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let _hello = ws.next().await.unwrap().unwrap();

    // Malformed but within the limit: answered with a JSON-RPC parse error
    // and the connection stays open.
    ws.send(Message::Text("x".repeat(LIMIT / 2).into()))
        .await
        .unwrap();
    let reply = ws.next().await.unwrap().unwrap();
    assert!(reply.to_text().unwrap().contains("Parse error"));
}
