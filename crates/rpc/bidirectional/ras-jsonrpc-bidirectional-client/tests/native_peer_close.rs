//! A server closing the connection releases the native transport, so the client can disconnect and reconnect.

#![cfg(all(feature = "native", not(target_arch = "wasm32")))]

use futures::SinkExt;
use ras_jsonrpc_bidirectional_client::{
    ClientBuilder, WebSocketTransport, client::Client, config::ClientConfig,
    native::NativeWebSocketTransport,
};
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionId};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// How the test server ends each connection after the handshake.
#[derive(Clone, Copy)]
enum Ending {
    CloseFrame,
    DropStream,
}

/// Accept `connections` WebSockets on a fresh port; each gets a
/// `ConnectionEstablished` message and is then ended as `ending` says.
async fn server(connections: usize, ending: Ending) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        for _ in 0..connections {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let hello = BidirectionalMessage::ConnectionEstablished {
                connection_id: ConnectionId::new(),
            };
            ws.send(Message::Text(serde_json::to_string(&hello).unwrap().into()))
                .await
                .unwrap();
            // Let the client finish connecting before the connection ends.
            tokio::time::sleep(Duration::from_millis(50)).await;
            match ending {
                Ending::CloseFrame => {
                    ws.send(Message::Close(None)).await.unwrap();
                    // Keep TCP alive so this exercises the Close branch, not
                    // an I/O error.
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        drop(ws);
                    });
                }
                Ending::DropStream => drop(ws),
            }
        }
    });
    (url, handle)
}

/// Receive until the transport reports the connection gone.
async fn receive_until_error(transport: &mut NativeWebSocketTransport) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            match transport.receive().await {
                Err(_) => break,
                Ok(_) => tokio::time::sleep(Duration::from_millis(1)).await,
            }
        }
    })
    .await
    .expect("a remote close must release the transport instead of waiting on itself");
}

#[tokio::test]
async fn peer_close_must_not_deadlock_the_native_transport() {
    let (url, peer) = server(1, Ending::CloseFrame).await;
    let mut transport = NativeWebSocketTransport::new(ClientConfig::new(url));
    transport.connect().await.unwrap();
    receive_until_error(&mut transport).await;
    peer.abort();
    assert!(!transport.is_connected());
    transport.disconnect().await.unwrap();
}

#[tokio::test]
async fn peer_dropping_the_stream_releases_the_native_transport() {
    let (url, peer) = server(1, Ending::DropStream).await;
    let mut transport = NativeWebSocketTransport::new(ClientConfig::new(url));
    transport.connect().await.unwrap();
    receive_until_error(&mut transport).await;
    peer.abort();
    tokio::time::timeout(Duration::from_millis(500), transport.disconnect())
        .await
        .expect("disconnect after a dropped stream must not hang")
        .unwrap();
}

async fn connected_client(url: &str) -> Client {
    let client = ClientBuilder::new(url)
        .with_heartbeat_interval(None)
        .build()
        .await
        .unwrap();
    client.connect().await.unwrap();
    client
}

#[tokio::test]
async fn client_recovers_after_the_server_closes() {
    let (url, peer) = server(2, Ending::CloseFrame).await;
    let client = connected_client(&url).await;

    tokio::time::timeout(Duration::from_secs(1), async {
        while client.is_connected().await {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the client notices the server's close");

    tokio::time::timeout(Duration::from_secs(1), client.disconnect())
        .await
        .expect("disconnect after a server close must not hang")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), client.connect())
        .await
        .expect("reconnect must not hang")
        .unwrap();
    assert!(client.is_connected().await);
    peer.abort();
}
