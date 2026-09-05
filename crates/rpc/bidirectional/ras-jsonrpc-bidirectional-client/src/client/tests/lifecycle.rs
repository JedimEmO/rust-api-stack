use super::*;

#[tokio::test]
async fn test_client_state() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("Failed to build client");

    assert_eq!(client.state().await, ClientState::Disconnected);
    assert!(!client.is_connected().await);
    assert!(client.connection_id().await.is_none());
}

#[tokio::test]
async fn disconnect_clears_pending_requests_connection_state_and_emits_event() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("build");
    *client.state.write().await = ClientState::Connected;
    *client.connection_id.write().await = Some(ConnectionId::new());

    let (message_tx, _message_rx) = mpsc::channel(1);
    *client.message_tx.write().await = Some(message_tx);
    let (pending_tx, pending_rx) = oneshot::channel();
    client.pending_requests.insert(
        serde_json::json!("in-flight"),
        PendingRequest {
            id: serde_json::json!("in-flight"),
            sender: pending_tx,
            created_at: Instant::now(),
        },
    );

    let events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let event_calls = std::sync::Arc::clone(&events);
    client.on_connection_event(
        "recorder",
        std::sync::Arc::new(move |event| {
            event_calls.lock().unwrap().push(event);
        }),
    );

    client.disconnect().await.expect("disconnect");

    assert_eq!(client.state().await, ClientState::Disconnected);
    assert!(client.connection_id().await.is_none());
    assert!(client.message_tx.read().await.is_none());
    assert!(client.pending_requests.is_empty());

    let failed_response = pending_rx.await.expect("pending waiter notified");
    assert_eq!(failed_response.id, Some(serde_json::json!("in-flight")));
    assert_eq!(
        failed_response.error.expect("disconnect error").code,
        ras_jsonrpc_types::error_codes::INTERNAL_ERROR
    );
    assert!(matches!(
        events.lock().unwrap().last().cloned().unwrap(),
        ConnectionEvent::Disconnected { reason: None }
    ));
}

#[tokio::test]
async fn connection_established_wakes_handshake_waiter() {
    let harness = std::sync::Arc::new(IncomingHarness::new());

    // Mirrors the wait loop in connect(): park on the notify until the
    // connection id is set. Without notify_one in the message handler
    // this would hang and the timeout below would fail the test.
    let waiter = {
        let harness = std::sync::Arc::clone(&harness);
        tokio::spawn(async move {
            loop {
                if harness.connection_id.read().await.is_some() {
                    break;
                }
                harness.connected_notify.notified().await;
            }
        })
    };

    tokio::task::yield_now().await;

    Client::handle_incoming_message(
        BidirectionalMessage::ConnectionEstablished {
            connection_id: ConnectionId::new(),
        },
        harness.context(),
    )
    .await;

    tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("handshake waiter woke up")
        .expect("waiter task completed");
}
