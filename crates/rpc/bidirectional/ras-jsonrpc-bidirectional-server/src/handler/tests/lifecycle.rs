use super::*;

#[tokio::test]
async fn default_lifecycle_methods_succeed() {
    let h = PassThrough;
    let c = ctx();
    h.on_connect(c.clone()).await.unwrap();
    h.on_ping(c.clone()).await.unwrap();
    h.on_pong(c.clone()).await.unwrap();
    h.on_disconnect(c.clone(), Some("bye".into()))
        .await
        .unwrap();
    // None reason path too.
    h.on_disconnect(c, None).await.unwrap();
}

#[tokio::test]
async fn handler_loop_processes_jsonrpc_request_without_socket() {
    let request = JsonRpcRequest::new(
        "echo".into(),
        Some(serde_json::json!({"value": 42})),
        Some(serde_json::json!(7)),
    );
    let incoming = serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap();
    let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text(incoming)]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(RespondingHandler), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));

    let response = match &messages[1] {
        BidirectionalMessage::Response(response) => response,
        other => panic!("expected response, got {other:?}"),
    };
    assert_eq!(response.id, Some(serde_json::json!(7)));
    assert_eq!(response.result.as_ref().unwrap()["method"], "echo");
    assert_eq!(response.result.as_ref().unwrap()["params"]["value"], 42);

    assert!(matches!(
        messages[2],
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}

#[tokio::test]
async fn handler_loop_processes_control_messages_without_socket() {
    let context = ctx();
    let subscribe = serde_json::to_string(&BidirectionalMessage::Subscribe {
        topics: vec!["room:1".into()],
    })
    .unwrap();
    let unsubscribe = serde_json::to_string(&BidirectionalMessage::Unsubscribe {
        topics: vec!["room:1".into()],
    })
    .unwrap();
    let mut socket = InMemorySocket::closing([
        WebSocketIoMessage::Text(subscribe),
        WebSocketIoMessage::Text(unsubscribe),
        WebSocketIoMessage::Ping(vec![1, 2, 3]),
    ]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(PassThrough), context.clone(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(!context.is_subscribed_to("room:1").await);
    assert!(
        socket
            .outgoing
            .contains(&WebSocketIoMessage::Pong(vec![1, 2, 3]))
    );
}

#[tokio::test]
async fn handler_loop_sends_manager_messages_without_socket() {
    let notification = BidirectionalMessage::ServerNotification(
        ras_jsonrpc_bidirectional_types::ServerNotification {
            method: "server.note".into(),
            params: serde_json::json!({"ok": true}),
            metadata: None,
        },
    );
    let (tx, rx) = mpsc::channel(4);
    tx.send(OutboundMessage::from(notification)).await.unwrap();
    drop(tx);

    let mut socket = InMemorySocket::pending();
    WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));

    match &messages[1] {
        BidirectionalMessage::ServerNotification(notification) => {
            assert_eq!(notification.method, "server.note");
            assert_eq!(notification.params["ok"], true);
        }
        other => panic!("expected server notification, got {other:?}"),
    }

    assert!(matches!(
        messages[2],
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}

#[tokio::test]
async fn handler_loop_records_close_reason_without_socket() {
    let handler = Arc::new(RecordingLifecycle::new());
    let mut socket =
        InMemorySocket::closing([WebSocketIoMessage::Close(Some("client bye".to_string()))]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(handler.clone(), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(
        handler
            .disconnect_reasons()
            .contains(&Some("client bye".to_string()))
    );
}
