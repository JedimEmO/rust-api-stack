use super::*;

#[tokio::test]
async fn handler_registration_does_not_require_connected_state() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("build");

    let n: NotificationHandler = std::sync::Arc::new(|_, _| {});
    let e: ConnectionEventHandler = std::sync::Arc::new(|_event| {});
    client.on_notification("evt", n);
    client.on_connection_event("named", e);

    // cleanup_expired_requests is callable even with nothing pending.
    client.cleanup_expired_requests().await;

    // Disconnect-when-already-disconnected is a no-op success.
    client.disconnect().await.expect("disconnect ok");
}

#[tokio::test]
async fn notify_subscribe_and_unsubscribe_send_expected_messages_when_connected() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("build");
    *client.state.write().await = ClientState::Connected;

    let (tx, mut rx) = mpsc::channel(4);
    *client.message_tx.write().await = Some(tx);

    client
        .notify("client.ready", Some(serde_json::json!({"ready": true})))
        .await
        .expect("notify");
    match rx.recv().await.expect("notify message") {
        BidirectionalMessage::Request(request) => {
            assert_eq!(request.method, "client.ready");
            assert_eq!(request.params, Some(serde_json::json!({"ready": true})));
            assert!(request.id.is_none());
        }
        other => panic!("unexpected notify message: {other:?}"),
    }

    let handler: NotificationHandler = std::sync::Arc::new(|_method, _params| {});
    client
        .subscribe("room:1", handler)
        .await
        .expect("subscribe");
    match rx.recv().await.expect("subscribe message") {
        BidirectionalMessage::Subscribe { topics } => {
            assert_eq!(topics, vec!["room:1".to_string()]);
        }
        other => panic!("unexpected subscribe message: {other:?}"),
    }
    assert_eq!(client.active_subscriptions(), vec!["room:1".to_string()]);

    client.unsubscribe("room:1").await.expect("unsubscribe");
    match rx.recv().await.expect("unsubscribe message") {
        BidirectionalMessage::Unsubscribe { topics } => {
            assert_eq!(topics, vec!["room:1".to_string()]);
        }
        other => panic!("unexpected unsubscribe message: {other:?}"),
    }
    assert!(client.active_subscriptions().is_empty());
}
