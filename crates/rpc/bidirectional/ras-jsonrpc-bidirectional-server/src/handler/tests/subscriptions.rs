use super::*;

#[tokio::test]
async fn default_handle_subscribe_denies_all_topics() {
    let h = PassThrough;
    let c = ctx();
    h.handle_subscribe(vec!["a".into(), "b".into()], c.clone())
        .await
        .unwrap();
    assert!(!c.is_subscribed_to("a").await);
    assert!(!c.is_subscribed_to("b").await);
}

#[tokio::test]
async fn default_authorize_subscribe_denies() {
    let h = PassThrough;
    let c = ctx();
    assert!(!h.authorize_subscribe("any-topic", &c).await.unwrap());
}

#[tokio::test]
async fn handle_subscribe_only_subscribes_authorized_topics() {
    let h = AllowListHandler;
    let c = ctx();
    h.handle_subscribe(vec!["room:allowed".into(), "room:denied".into()], c.clone())
        .await
        .unwrap();
    assert!(c.is_subscribed_to("room:allowed").await);
    assert!(!c.is_subscribed_to("room:denied").await);
}

#[tokio::test]
async fn default_handle_unsubscribe_removes_from_context() {
    let h = PassThrough;
    let c = ctx();
    c.subscribe("a".into()).await.unwrap();
    c.subscribe("b".into()).await.unwrap();
    h.handle_unsubscribe(vec!["a".into()], c.clone())
        .await
        .unwrap();
    assert!(!c.is_subscribed_to("a").await);
    assert!(c.is_subscribed_to("b").await);
}

#[tokio::test]
async fn w3_subscribe_over_per_message_limit_is_rejected() {
    let context = ctx_with(limits_policy(SubscriptionLimits {
        max_topics_per_message: 2,
        ..SubscriptionLimits::default()
    }));
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let (_tx, rx) = mpsc::channel(4);
    let topics: Vec<String> = (0..3).map(|i| format!("room:{i}")).collect();
    let mut socket = InMemorySocket::closing([subscribe_msg(topics)]);

    WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(context.get_subscriptions().await.is_empty());
    assert!(bidirectional_outgoing(&socket).iter().any(|m| matches!(
        m,
        BidirectionalMessage::Response(r) if r.error.is_some()
    )));
}

#[tokio::test]
async fn w3_subscribe_over_per_connection_limit_is_rejected() {
    let context = ctx_with(limits_policy(SubscriptionLimits {
        max_topics_per_connection: 1,
        ..SubscriptionLimits::default()
    }));
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::closing([
        subscribe_msg(vec!["room:1".into()]),
        subscribe_msg(vec!["room:2".into()]),
    ]);

    WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    // First subscribe accepted silently; second answered with an error.
    let errors = bidirectional_outgoing(&socket)
        .iter()
        .filter(|m| matches!(m, BidirectionalMessage::Response(r) if r.error.is_some()))
        .count();
    assert_eq!(errors, 1);
    // Teardown released the held slot.
    assert_eq!(context.subscription_policy().accounting.total(), 0);
    assert!(context.get_subscriptions().await.is_empty());

    // Direct path: the context itself refuses the second topic.
    let direct = ctx_with(limits_policy(SubscriptionLimits {
        max_topics_per_connection: 1,
        ..SubscriptionLimits::default()
    }));
    direct.subscribe("room:1".into()).await.unwrap();
    assert!(matches!(
        direct.subscribe("room:2".into()).await,
        Err(ras_jsonrpc_bidirectional_types::BidirectionalError::SubscriptionLimitReached(_))
    ));
}

#[tokio::test]
async fn w3_overlong_topic_is_rejected() {
    let context = ctx();
    context.set_user(auth_user_with("u", &["room:read"])).await;
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::closing([subscribe_msg(vec!["x".repeat(300)])]);

    WebSocketHandler::new(Arc::new(PermissionGated), context.clone(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(context.get_subscriptions().await.is_empty());
}

#[tokio::test]
async fn w3_global_subscription_cap_is_enforced_across_connections() {
    // Service-level accounting shared by both contexts. The first
    // connection stays open (pending socket) so its slots remain held
    // while the second connection tries to subscribe.
    let limits = SubscriptionLimits {
        max_total_subscriptions: 2,
        ..SubscriptionLimits::default()
    };
    let accounting = Arc::new(SubscriptionAccounting::default());
    let policy = crate::connection::SubscriptionPolicy {
        limits,
        accounting: accounting.clone(),
        manager: None,
    };

    let first = ctx_with(policy.clone());
    first.set_user(auth_user_with("u", &["room:read"])).await;
    let (_tx1, rx1) = mpsc::channel(4);
    let mut socket1 = InMemorySocket::pending();
    socket1
        .incoming
        .push_back(subscribe_msg(vec!["a".into(), "b".into()]));
    let first_run = {
        let first = first.clone();
        tokio::spawn(async move {
            WebSocketHandler::new(Arc::new(PermissionGated), first, rx1, 1024)
                .with_keepalive(KeepaliveConfig {
                    ping_interval: None,
                    idle_timeout: None,
                })
                .run_with_io(&mut socket1)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        while accounting.total() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first connection should reserve its 2 slots");
    assert_eq!(first.get_subscriptions().await.len(), 2);

    let second = ctx_with(policy);
    second.set_user(auth_user_with("u", &["room:read"])).await;
    let (_tx2, rx2) = mpsc::channel(4);
    let mut socket2 = InMemorySocket::closing([subscribe_msg(vec!["c".into()])]);
    WebSocketHandler::new(Arc::new(PermissionGated), second.clone(), rx2, 1024)
        .run_with_io(&mut socket2)
        .await
        .unwrap();

    assert!(second.get_subscriptions().await.is_empty());
    assert_eq!(accounting.total(), 2, "second connection reserved nothing");
    first_run.abort();
}

#[tokio::test]
async fn w3_custom_handler_cannot_exceed_limits_from_any_callback() {
    let limits = SubscriptionLimits {
        max_topics_per_connection: 3,
        ..SubscriptionLimits::default()
    };
    let manager: Arc<dyn ConnectionManager> = Arc::new(
        crate::DefaultConnectionManager::with_subscription_limits(limits),
    );
    let accounting = Arc::new(SubscriptionAccounting::default());
    let context = ctx_with(crate::connection::SubscriptionPolicy {
        limits,
        accounting: accounting.clone(),
        manager: Some(manager.clone()),
    });
    manager
        .add_connection(ras_jsonrpc_bidirectional_types::ConnectionInfo::new(
            context.id,
        ))
        .await
        .unwrap();
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::closing([subscribe_msg(vec!["x".into()])]);

    WebSocketHandler::new(Arc::new(GreedyHandler), context.clone(), rx, 1024)
        .with_connection_manager(manager.clone())
        .run_with_io(&mut socket)
        .await
        .unwrap();

    // Greedy on_connect (10), handle_request-free, then greedy
    // handle_subscribe (10 more): the context admitted three in total,
    // the manager saw exactly those, and disconnect released exactly
    // those, so the counter is back to zero, not underflowed.
    assert_eq!(
        context.get_subscriptions().await.len(),
        0,
        "released on disconnect"
    );
    assert_eq!(
        manager.get_subscriptions(context.id).await.unwrap().len(),
        0
    );
    assert_eq!(accounting.total(), 0, "no underflow");
    assert_eq!(manager.total_subscription_count().await.unwrap(), 0);
    assert_eq!(
        GREEDY_ACCEPTED.load(std::sync::atomic::Ordering::SeqCst),
        3,
        "exactly the cap accepted across on_connect and handle_subscribe"
    );
}
