use super::*;

#[tokio::test]
async fn incoming_response_delivers_to_matching_pending_request() {
    let harness = IncomingHarness::new();
    let request_id = serde_json::json!(42);
    let (tx, rx) = oneshot::channel();
    harness.pending_requests.insert(
        request_id.clone(),
        PendingRequest {
            id: request_id.clone(),
            sender: tx,
            created_at: Instant::now(),
        },
    );

    Client::handle_incoming_message(
        BidirectionalMessage::Response(JsonRpcResponse::success(
            serde_json::json!({"ok": true}),
            Some(request_id),
        )),
        harness.context(),
    )
    .await;

    assert!(harness.pending_requests.is_empty());
    let response = rx.await.expect("pending response delivered");
    assert_eq!(response.result, Some(serde_json::json!({"ok": true})));

    Client::handle_incoming_message(
        BidirectionalMessage::Response(JsonRpcResponse::success(
            serde_json::json!("ignored"),
            Some(serde_json::json!("unknown")),
        )),
        harness.context(),
    )
    .await;
    Client::handle_incoming_message(
        BidirectionalMessage::Response(JsonRpcResponse::success(
            serde_json::json!("notification-like"),
            None,
        )),
        harness.context(),
    )
    .await;
}

#[tokio::test]
async fn incoming_notifications_and_broadcasts_route_to_registered_handlers() {
    let harness = IncomingHarness::new();
    let notifications = std::sync::Arc::new(Mutex::new(Vec::new()));
    let broadcasts = std::sync::Arc::new(Mutex::new(Vec::new()));

    let notification_calls = std::sync::Arc::clone(&notifications);
    harness.notification_handlers.insert(
        "server.event".to_string(),
        std::sync::Arc::new(move |method, params| {
            notification_calls
                .lock()
                .unwrap()
                .push((method.to_string(), params.clone()));
        }),
    );

    let broadcast_calls = std::sync::Arc::clone(&broadcasts);
    harness.subscriptions.insert(
        "room:1".to_string(),
        Subscription {
            topic: "room:1".to_string(),
            handler: std::sync::Arc::new(move |method, params| {
                broadcast_calls
                    .lock()
                    .unwrap()
                    .push((method.to_string(), params.clone()));
            }),
            created_at: Instant::now(),
        },
    );

    Client::handle_incoming_message(
        BidirectionalMessage::ServerNotification(
            ras_jsonrpc_bidirectional_types::ServerNotification {
                method: "server.event".to_string(),
                params: serde_json::json!({"n": 1}),
                metadata: None,
            },
        ),
        harness.context(),
    )
    .await;
    Client::handle_incoming_message(
        BidirectionalMessage::Broadcast(ras_jsonrpc_bidirectional_types::BroadcastMessage {
            topic: "room:1".to_string(),
            method: "chat.message".to_string(),
            params: serde_json::json!({"body": "hi"}),
            metadata: None,
        }),
        harness.context(),
    )
    .await;
    Client::handle_incoming_message(
        BidirectionalMessage::Broadcast(ras_jsonrpc_bidirectional_types::BroadcastMessage {
            topic: "room:2".to_string(),
            method: "chat.message".to_string(),
            params: serde_json::json!({"body": "ignored"}),
            metadata: None,
        }),
        harness.context(),
    )
    .await;

    assert_eq!(
        *notifications.lock().unwrap(),
        vec![("server.event".to_string(), serde_json::json!({"n": 1}))]
    );
    assert_eq!(
        *broadcasts.lock().unwrap(),
        vec![(
            "chat.message".to_string(),
            serde_json::json!({"body": "hi"})
        )]
    );
}

#[tokio::test]
async fn incoming_connection_lifecycle_updates_id_and_emits_events() {
    let harness = IncomingHarness::new();
    let events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let event_calls = std::sync::Arc::clone(&events);
    harness.connection_event_handlers.insert(
        "recorder".to_string(),
        std::sync::Arc::new(move |event| {
            event_calls.lock().unwrap().push(event);
        }),
    );

    let id = ConnectionId::new();
    Client::handle_incoming_message(
        BidirectionalMessage::ConnectionEstablished { connection_id: id },
        harness.context(),
    )
    .await;

    assert_eq!(*harness.connection_id.read().await, Some(id));
    let first_event = events.lock().unwrap().first().cloned().unwrap();
    assert!(matches!(
        first_event,
        ConnectionEvent::Connected { connection_id } if connection_id == id
    ));

    Client::handle_incoming_message(
        BidirectionalMessage::ConnectionClosed {
            connection_id: id,
            reason: Some("server shutdown".to_string()),
        },
        harness.context(),
    )
    .await;

    assert!(harness.connection_id.read().await.is_none());
    let last_event = events.lock().unwrap().last().cloned().unwrap();
    assert!(matches!(
        last_event,
        ConnectionEvent::Disconnected { reason: Some(reason) } if reason == "server shutdown"
    ));
}

#[tokio::test]
async fn incoming_rpc_request_sends_handler_response_or_method_not_found() {
    let harness = IncomingHarness::new();
    let (tx, mut rx) = mpsc::channel(4);
    *harness.message_tx.write().await = Some(tx);

    let handler: RpcRequestHandler = std::sync::Arc::new(|request| {
        Box::pin(async move {
            JsonRpcResponse::success(
                serde_json::json!({ "handled": request.method }),
                request.id.clone(),
            )
        })
    });
    harness
        .rpc_request_handlers
        .insert("client.echo".to_string(), handler);

    Client::handle_incoming_message(
        BidirectionalMessage::Request(JsonRpcRequest::new(
            "client.echo".to_string(),
            None,
            Some(serde_json::json!("known")),
        )),
        harness.context(),
    )
    .await;

    let response = rx.recv().await.expect("handler response sent");
    match response {
        BidirectionalMessage::Response(response) => {
            assert_eq!(response.id, Some(serde_json::json!("known")));
            assert_eq!(
                response.result,
                Some(serde_json::json!({"handled": "client.echo"}))
            );
        }
        other => panic!("unexpected outgoing message: {other:?}"),
    }

    Client::handle_incoming_message(
        BidirectionalMessage::Request(JsonRpcRequest::new(
            "client.missing".to_string(),
            None,
            Some(serde_json::json!("missing")),
        )),
        harness.context(),
    )
    .await;

    let response = rx.recv().await.expect("method-not-found response sent");
    match response {
        BidirectionalMessage::Response(response) => {
            assert_eq!(response.id, Some(serde_json::json!("missing")));
            let error = response.error.expect("error response");
            assert_eq!(error.code, ras_jsonrpc_types::error_codes::METHOD_NOT_FOUND);
            assert_eq!(error.message, "Method not found");
        }
        other => panic!("unexpected outgoing message: {other:?}"),
    }

    Client::handle_incoming_message(
        BidirectionalMessage::Request(JsonRpcRequest::new("client.echo".to_string(), None, None)),
        harness.context(),
    )
    .await;

    assert!(rx.try_recv().is_err());
}
