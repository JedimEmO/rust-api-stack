use super::*;

#[tokio::test(start_paused = true)]
async fn revalidation_failure_closes_connection() {
    let context = ctx();
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::pending();

    WebSocketHandler::new(Arc::new(PassThrough), context, rx, 1024)
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(SequenceAuthProvider::new([])),
            token: "revoked-token".into(),
            interval: Duration::from_secs(30),
            on_permission_change: PermissionChangePolicy::default(),
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(socket.outgoing.iter().any(|message| matches!(
        message,
        WebSocketIoMessage::Close(Some(reason)) if reason == "credentials no longer valid"
    )));
}

#[tokio::test(start_paused = true)]
async fn revalidation_success_refreshes_cached_user() {
    let context = ctx();
    context.set_user(auth_user("stale")).await;
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::pending();

    WebSocketHandler::new(Arc::new(PassThrough), context.clone(), rx, 1024)
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(SequenceAuthProvider::new([Ok(auth_user("fresh"))])),
            token: "valid-token".into(),
            interval: Duration::from_secs(30),
            on_permission_change: PermissionChangePolicy::default(),
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    // First tick refreshed the cached user; the second (sequence
    // exhausted) failed and closed the connection.
    assert_eq!(context.get_user().await.expect("user").user_id, "fresh");
    assert!(
        socket
            .outgoing
            .iter()
            .any(|message| matches!(message, WebSocketIoMessage::Close(_)))
    );
}

#[tokio::test(start_paused = true)]
async fn w1_revalidation_drops_subscriptions_no_longer_authorized() {
    let context = ctx();
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let manager = manager_with(&context).await;
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::pending();
    socket
        .incoming
        .push_back(subscribe_msg(vec!["room:1".into()]));

    // Tick 1 returns the same user with the permission revoked; tick 2 fails.
    let provider = SequenceAuthProvider::new([Ok(auth_user_with("u", &[]))]);
    WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
        .with_connection_manager(manager.clone())
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(provider),
            token: "t".into(),
            interval: Duration::from_secs(30),
            on_permission_change: PermissionChangePolicy::DropSubscriptions,
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(!context.is_subscribed_to("room:1").await);
    assert!(
        manager
            .get_subscriptions(context.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        manager
            .get_subscribed_connections("room:1")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn w1_subscribe_mirrors_into_manager_index() {
    let manager: Arc<dyn ConnectionManager> = Arc::new(crate::DefaultConnectionManager::new());
    let context = ctx_with(crate::connection::SubscriptionPolicy {
        manager: Some(manager.clone()),
        ..Default::default()
    });
    manager
        .add_connection(ras_jsonrpc_bidirectional_types::ConnectionInfo::new(
            context.id,
        ))
        .await
        .unwrap();

    context.subscribe("room:1".into()).await.unwrap();
    assert!(context.is_subscribed_to("room:1").await);
    assert_eq!(
        manager.get_subscriptions(context.id).await.unwrap(),
        vec!["room:1".to_string()]
    );

    assert!(context.unsubscribe("room:1").await);
    assert!(
        manager
            .get_subscriptions(context.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(context.subscription_policy().accounting.total(), 0);
}

#[tokio::test(start_paused = true)]
async fn w1_close_policy_closes_socket_when_permissions_change() {
    let context = ctx();
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::pending();

    WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(SequenceAuthProvider::new([Ok(auth_user_with("u", &[]))])),
            token: "t".into(),
            interval: Duration::from_secs(30),
            on_permission_change: PermissionChangePolicy::Close,
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    // Closed on the first tick (permission change), not the second (failure).
    assert_eq!(
        socket
            .outgoing
            .iter()
            .filter(|m| matches!(m, WebSocketIoMessage::Close(_)))
            .count(),
        1
    );
    assert!(context.get_user().await.unwrap().permissions.is_empty());
}

#[tokio::test(start_paused = true)]
async fn w1_broadcast_queued_during_revocation_window_is_not_delivered() {
    let context = ctx();
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let manager = manager_with(&context).await;
    let (tx, rx) = mpsc::channel(4);
    // The handler's context must share this channel so the authorizer
    // can enqueue through `context.sender`.
    let context = Arc::new(ConnectionContext::new(
        context.id,
        ChannelMessageSender::new(context.id, tx),
    ));
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let mut socket = InMemorySocket::pending();
    socket
        .incoming
        .push_back(subscribe_msg(vec!["room:1".into()]));

    WebSocketHandler::new(Arc::new(RacingAuthorizer), context.clone(), rx, 1024)
        .with_connection_manager(manager)
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(SequenceAuthProvider::new([Ok(auth_user_with("u", &[]))])),
            token: "t".into(),
            interval: Duration::from_secs(30),
            on_permission_change: PermissionChangePolicy::DropSubscriptions,
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(!context.is_subscribed_to("room:1").await);
    let leaked = bidirectional_outgoing(&socket)
        .iter()
        .any(|m| matches!(m, BidirectionalMessage::Broadcast(b) if b.method == "secret"));
    assert!(
        !leaked,
        "broadcast queued during the revocation window must be dropped"
    );
}

#[tokio::test]
async fn w1_egress_gate_only_filters_topic_routed_messages() {
    let context = ctx();
    let (tx, rx) = mpsc::channel(4);
    let ping = BidirectionalMessage::Ping;
    tx.send(OutboundMessage::from(ping.clone())).await.unwrap();
    tx.send(OutboundMessage {
        message: ping,
        topic: Some("room:never".into()),
    })
    .await
    .unwrap();
    drop(tx);

    let mut socket = InMemorySocket::pending();
    WebSocketHandler::new(Arc::new(PassThrough), context, rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let pings = bidirectional_outgoing(&socket)
        .iter()
        .filter(|m| matches!(m, BidirectionalMessage::Ping))
        .count();
    assert_eq!(
        pings, 1,
        "untagged delivered, topic-tagged unsubscribed dropped"
    );
}

#[tokio::test]
async fn handler_without_revalidation_does_not_authenticate() {
    // No auth provider involved at all: the loop must terminate on
    // socket close without ticking a revalidation timer.
    let mut socket = InMemorySocket::closing([]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert_eq!(messages.len(), 2);
}
