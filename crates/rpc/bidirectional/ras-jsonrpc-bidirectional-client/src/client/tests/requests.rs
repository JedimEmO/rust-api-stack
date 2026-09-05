use super::*;

#[tokio::test]
async fn call_notify_subscribe_unsubscribe_require_connected_state() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("build");

    // call → NotConnected
    let err = client.call("m", None).await.unwrap_err();
    assert!(matches!(err, ClientError::NotConnected));

    // notify → NotConnected
    let err = client.notify("m", None).await.unwrap_err();
    assert!(matches!(err, ClientError::NotConnected));

    // subscribe → NotConnected
    let handler: NotificationHandler = std::sync::Arc::new(|_method: &str, _params: &Value| {});
    let err = client.subscribe("t", handler.clone()).await.unwrap_err();
    assert!(matches!(err, ClientError::NotConnected));

    // unsubscribe → NotConnected
    let err = client.unsubscribe("t").await.unwrap_err();
    assert!(matches!(err, ClientError::NotConnected));
}

#[tokio::test]
async fn call_sends_request_and_completes_when_pending_response_arrives() {
    let client = std::sync::Arc::new(
        ClientBuilder::new("ws://localhost:8080")
            .build()
            .await
            .expect("build"),
    );
    *client.state.write().await = ClientState::Connected;

    let (tx, mut rx) = mpsc::channel(4);
    *client.message_tx.write().await = Some(tx);

    let call_task = {
        let client = std::sync::Arc::clone(&client);
        tokio::spawn(async move {
            client
                .call("svc.echo", Some(serde_json::json!({"input": 1})))
                .await
        })
    };

    let request_id = match rx.recv().await.expect("outgoing request") {
        BidirectionalMessage::Request(request) => {
            assert_eq!(request.method, "svc.echo");
            assert_eq!(request.params, Some(serde_json::json!({"input": 1})));
            request.id.expect("request id")
        }
        other => panic!("unexpected outgoing request: {other:?}"),
    };

    let (_, pending) = client
        .pending_requests
        .remove(&request_id)
        .expect("pending request registered");
    pending
        .sender
        .send(JsonRpcResponse::success(
            serde_json::json!({"output": 1}),
            Some(request_id),
        ))
        .expect("deliver response");

    let response = call_task.await.expect("join").expect("call response");
    assert_eq!(response.result, Some(serde_json::json!({"output": 1})));
    assert!(client.pending_requests.is_empty());
}

#[tokio::test]
async fn call_returns_internal_error_when_pending_request_limit_is_reached() {
    let mut config = ClientConfig::new("ws://localhost:8080");
    config.max_pending_requests = 1;
    let client = Client::new(config).await.expect("client");
    *client.state.write().await = ClientState::Connected;

    let (message_tx, mut message_rx) = mpsc::channel(1);
    *client.message_tx.write().await = Some(message_tx);
    let (pending_tx, _pending_rx) = oneshot::channel();
    client.pending_requests.insert(
        serde_json::json!("existing"),
        PendingRequest {
            id: serde_json::json!("existing"),
            sender: pending_tx,
            created_at: Instant::now(),
        },
    );

    let err = client.call("svc.echo", None).await.unwrap_err();
    assert!(
        matches!(err, ClientError::Internal(message) if message == "Too many pending requests")
    );
    assert!(message_rx.try_recv().is_err());
    assert_eq!(client.pending_requests.len(), 1);
}

#[tokio::test]
async fn cleanup_expired_requests_removes_expired_waiters_and_keeps_fresh_ones() {
    let mut config = ClientConfig::new("ws://localhost:8080");
    config.request_timeout = Duration::from_secs(1);
    let client = Client::new(config).await.expect("client");

    let (expired_tx, expired_rx) = oneshot::channel();
    client.pending_requests.insert(
        serde_json::json!("expired"),
        PendingRequest {
            id: serde_json::json!("expired"),
            sender: expired_tx,
            created_at: Instant::now() - Duration::from_secs(5),
        },
    );

    let (fresh_tx, _fresh_rx) = oneshot::channel();
    client.pending_requests.insert(
        serde_json::json!("fresh"),
        PendingRequest {
            id: serde_json::json!("fresh"),
            sender: fresh_tx,
            created_at: Instant::now(),
        },
    );

    client.cleanup_expired_requests().await;

    let timeout_response = expired_rx.await.expect("expired waiter notified");
    assert_eq!(timeout_response.id, Some(serde_json::json!("expired")));
    assert_eq!(
        timeout_response.error.expect("timeout error").code,
        ras_jsonrpc_types::error_codes::INTERNAL_ERROR
    );
    assert!(
        !client
            .pending_requests
            .contains_key(&serde_json::json!("expired"))
    );
    assert!(
        client
            .pending_requests
            .contains_key(&serde_json::json!("fresh"))
    );
}

#[tokio::test(start_paused = true)]
async fn call_timeout_removes_pending_entry_and_allows_retry() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .with_request_timeout(Duration::from_millis(20))
        .build()
        .await
        .expect("build");
    *client.state.write().await = ClientState::Connected;

    let (tx, mut rx) = mpsc::channel(8);
    *client.message_tx.write().await = Some(tx);

    let err = client.call("svc.slow", None).await.unwrap_err();
    assert!(matches!(err, ClientError::Timeout { .. }));
    assert!(
        client.pending_requests.is_empty(),
        "timed-out call must remove its pending entry"
    );
    let _ = rx.recv().await;

    // The map must not fill up with dead waiters: a retry times out
    // again rather than failing with "Too many pending requests".
    let err = client.call("svc.slow", None).await.unwrap_err();
    assert!(matches!(err, ClientError::Timeout { .. }));
    assert!(client.pending_requests.is_empty());
}

#[tokio::test]
async fn call_send_failure_removes_pending_entry() {
    let client = ClientBuilder::new("ws://localhost:8080")
        .build()
        .await
        .expect("build");
    *client.state.write().await = ClientState::Connected;

    // Install a sender whose receiver is already gone so send fails.
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    *client.message_tx.write().await = Some(tx);

    let err = client.call("svc.echo", None).await.unwrap_err();
    assert!(!matches!(err, ClientError::Timeout { .. }));
    assert!(
        client.pending_requests.is_empty(),
        "failed send must remove its pending entry"
    );
}
