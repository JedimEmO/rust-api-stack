use super::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::BidirectionalError;
use std::sync::Arc;
use tokio::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[tokio::test]
async fn test_message_sender_ext() {
    // Create a mock sender
    struct MockSender {
        connection_id: ConnectionId,
        sent_messages: Arc<Mutex<Vec<BidirectionalMessage>>>,
    }

    #[async_trait]
    impl MessageSender for MockSender {
        async fn send_message(&self, message: BidirectionalMessage) -> Result<()> {
            self.sent_messages.lock().await.push(message);
            Ok(())
        }

        async fn close(&self) -> Result<()> {
            Ok(())
        }

        async fn is_connected(&self) -> bool {
            true
        }

        fn connection_id(&self) -> ConnectionId {
            self.connection_id
        }
    }

    let sender = MockSender {
        connection_id: ConnectionId::new(),
        sent_messages: Arc::new(Mutex::new(Vec::new())),
    };

    // Test convenience methods
    sender.send_ping().await.unwrap();
    sender.send_pong().await.unwrap();
    sender
        .send_notification("test.method", serde_json::json!({"key": "value"}))
        .await
        .unwrap();

    let messages = sender.sent_messages.lock().await;
    assert_eq!(messages.len(), 3);

    // Check message types
    assert!(matches!(messages[0], BidirectionalMessage::Ping));
    assert!(matches!(messages[1], BidirectionalMessage::Pong));
    assert!(matches!(
        &messages[2],
        BidirectionalMessage::ServerNotification(n) if n.method == "test.method"
    ));
}

#[tokio::test]
async fn message_sender_ext_request_response_subscription() {
    struct Recorder {
        id: ConnectionId,
        sent: Arc<Mutex<Vec<BidirectionalMessage>>>,
    }
    #[async_trait]
    impl MessageSender for Recorder {
        async fn send_message(&self, message: BidirectionalMessage) -> Result<()> {
            self.sent.lock().await.push(message);
            Ok(())
        }
        async fn close(&self) -> Result<()> {
            Ok(())
        }
        async fn is_connected(&self) -> bool {
            true
        }
        fn connection_id(&self) -> ConnectionId {
            self.id
        }
    }
    let r = Recorder {
        id: ConnectionId::new(),
        sent: Arc::new(Mutex::new(Vec::new())),
    };

    r.send_request(ras_jsonrpc_types::JsonRpcRequest {
        jsonrpc: "2.0".into(),
        method: "m".into(),
        params: None,
        id: Some(serde_json::json!(1)),
    })
    .await
    .unwrap();
    r.send_response(ras_jsonrpc_types::JsonRpcResponse::success(
        serde_json::json!("ok"),
        Some(serde_json::json!(1)),
    ))
    .await
    .unwrap();
    r.send_subscription_update(vec!["t1".into()], true)
        .await
        .unwrap();
    r.send_subscription_update(vec!["t1".into()], false)
        .await
        .unwrap();

    let s = r.sent.lock().await;
    assert!(matches!(s[0], BidirectionalMessage::Request(_)));
    assert!(matches!(s[1], BidirectionalMessage::Response(_)));
    assert!(matches!(s[2], BidirectionalMessage::Subscribe { .. }));
    assert!(matches!(s[3], BidirectionalMessage::Unsubscribe { .. }));
}

#[tokio::test]
async fn noop_message_sender_round_trip() {
    let id = ConnectionId::new();
    let sender = NoOpMessageSender::with_connection_id(id);
    assert_eq!(sender.connection_id(), id);
    assert!(sender.is_connected().await);
    sender
        .send_message(BidirectionalMessage::Ping)
        .await
        .unwrap();
    sender.close().await.unwrap();

    // Default constructor + Default impl.
    let s2 = NoOpMessageSender::new();
    let s3 = NoOpMessageSender::default();
    assert_ne!(s2.connection_id(), s3.connection_id());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn websocket_sender_drives_real_sink() {
    use futures::channel::mpsc;
    use futures::stream::StreamExt;

    // mpsc::channel's Sender impls Sink<T>, satisfying the SinkExt bound
    // on `WebSocketMessageSender::new`.
    let (tx, mut rx) = mpsc::channel::<WsMessage>(8);
    let id = ConnectionId::new();
    let sender = WebSocketMessageSender::new(id, tx);

    assert_eq!(sender.connection_id(), id);
    assert!(sender.is_connected().await);

    sender
        .send_message(BidirectionalMessage::Ping)
        .await
        .unwrap();
    // close once → emits a Close frame and flips is_closed.
    sender.close().await.unwrap();
    assert!(!sender.is_connected().await);
    // close again is idempotent (no panic, no extra send).
    sender.close().await.unwrap();

    // Sending after close yields ConnectionClosed.
    let err = sender
        .send_message(BidirectionalMessage::Pong)
        .await
        .unwrap_err();
    assert!(matches!(err, BidirectionalError::ConnectionClosed));

    // Drain what we actually pushed: a Text(Ping) and a Close.
    let mut received: Vec<WsMessage> = Vec::new();
    while let Some(m) = rx.next().await {
        received.push(m);
        if received.len() == 2 {
            break;
        }
    }
    assert_eq!(received.len(), 2);
    match &received[0] {
        WsMessage::Text(t) => assert!(t.contains("ping")),
        other => panic!("expected Text(ping), got {other:?}"),
    }
    assert!(matches!(received[1], WsMessage::Close(_)));
}
