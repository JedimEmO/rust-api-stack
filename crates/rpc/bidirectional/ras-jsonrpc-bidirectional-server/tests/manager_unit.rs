//! Direct unit tests for `DefaultConnectionManager`.
//!
//! The socketless generated-handler suite in `ras-jsonrpc-bidirectional-macro`
//! covers the manager's happy path indirectly. This file pins down the
//! manager's contract on its own: subscriptions, broadcast counts, permission
//! filtering, and pending-request lifecycle, without spinning up a real
//! WebSocket.

use std::collections::HashSet;
use std::sync::Arc;

use ras_auth_core::AuthenticatedUser;
use ras_jsonrpc_bidirectional_server::DefaultConnectionManager;
use ras_jsonrpc_bidirectional_server::connection::ChannelMessageSender;
use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, ConnectionId, ConnectionInfo, ConnectionManager,
};
use ras_jsonrpc_types::JsonRpcResponse;
use tokio::sync::{mpsc, oneshot};

fn user(id: &str, perms: &[&str]) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: id.to_string(),
        permissions: perms.iter().map(|s| s.to_string()).collect::<HashSet<_>>(),
        metadata: None,
    }
}

/// Build a connection paired with a real receiver so we can observe sends.
async fn join(
    mgr: &DefaultConnectionManager,
) -> (ConnectionId, mpsc::Receiver<BidirectionalMessage>) {
    let id = ConnectionId::new();
    let (tx, rx) = mpsc::channel(16);
    let sender = ChannelMessageSender::new(id, tx);
    let info = ConnectionInfo::new(id);
    mgr.add_connection_with_sender_direct(info, sender)
        .await
        .unwrap();
    (id, rx)
}

#[tokio::test]
async fn add_remove_round_trip_and_inspect() {
    let mgr = DefaultConnectionManager::new();
    assert_eq!(mgr.connection_count(), 0);

    let (a, _ra) = join(&mgr).await;
    let (b, _rb) = join(&mgr).await;
    assert_eq!(mgr.connection_count(), 2);

    let ids = mgr.get_connection_ids();
    assert!(ids.contains(&a) && ids.contains(&b));
    assert!(mgr.connection_exists(a).await.unwrap());
    assert!(mgr.get_sender(a).is_some());

    // Removing a missing id is logged-and-ignored, not an error.
    mgr.remove_connection(ConnectionId::new()).await.unwrap();

    mgr.remove_connection(a).await.unwrap();
    assert_eq!(mgr.connection_count(), 1);
    assert!(!mgr.connection_exists(a).await.unwrap());
    assert!(mgr.get_sender(a).is_none());
}

#[tokio::test]
async fn add_connection_with_sender_box_downcasts() {
    let mgr = DefaultConnectionManager::new();
    let id = ConnectionId::new();
    let (tx, _rx) = mpsc::channel(1);
    let sender = ChannelMessageSender::new(id, tx);
    // Round-trip through Box<dyn Any> as the trait method requires.
    let boxed: Box<dyn std::any::Any + Send + Sync> = Box::new(sender);
    mgr.add_connection_with_sender(ConnectionInfo::new(id), boxed)
        .await
        .unwrap();
    assert!(mgr.connection_exists(id).await.unwrap());
    assert!(mgr.get_sender(id).is_some());
}

#[tokio::test]
async fn add_connection_with_unknown_sender_uses_fallback_channel() {
    let mgr = DefaultConnectionManager::new();
    let id = ConnectionId::new();
    let unexpected_sender: Box<dyn std::any::Any + Send + Sync> = Box::new(123u32);
    mgr.add_connection_with_sender(ConnectionInfo::new(id), unexpected_sender)
        .await
        .unwrap();
    assert!(mgr.connection_exists(id).await.unwrap());
}

#[tokio::test]
async fn subscriptions_track_topics_and_clean_up_on_remove() {
    let mgr = DefaultConnectionManager::new();
    let (a, _ra) = join(&mgr).await;
    let (b, _rb) = join(&mgr).await;

    mgr.add_subscription(a, "room:1".into()).await.unwrap();
    mgr.add_subscription(b, "room:1".into()).await.unwrap();
    mgr.add_subscription(b, "room:2".into()).await.unwrap();

    let topics = mgr.get_active_topics();
    assert!(topics.contains(&"room:1".to_string()));
    assert!(topics.contains(&"room:2".to_string()));

    let r1: HashSet<_> = mgr.get_topic_connections("room:1").into_iter().collect();
    assert!(r1.contains(&a) && r1.contains(&b));

    let subs_b = mgr.get_subscriptions(b).await.unwrap();
    assert!(subs_b.iter().any(|s| s == "room:1"));
    assert!(subs_b.iter().any(|s| s == "room:2"));

    // Direct unsubscribe on the only-non-empty topic frees it.
    mgr.remove_subscription(a, "room:1").await.unwrap();
    mgr.remove_subscription(b, "room:1").await.unwrap();
    assert!(!mgr.get_active_topics().contains(&"room:1".to_string()));

    // Removing a connection prunes any remaining subscriptions for it.
    mgr.remove_connection(b).await.unwrap();
    // room:2 had only b, so it should be gone.
    assert!(!mgr.get_active_topics().contains(&"room:2".to_string()));
}

#[tokio::test]
async fn subscribed_connections_returns_full_info_for_topic() {
    let mgr = DefaultConnectionManager::new();
    let (a, _ra) = join(&mgr).await;
    let (_b, _rb) = join(&mgr).await;
    mgr.add_subscription(a, "t".into()).await.unwrap();
    let subs = mgr.get_subscribed_connections("t").await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].id, a);
}

#[tokio::test]
async fn user_set_clear_filter_paths_for_broadcasts() {
    let mgr = DefaultConnectionManager::new();
    let (auth_id, mut auth_rx) = join(&mgr).await;
    let (admin_id, mut admin_rx) = join(&mgr).await;
    let (_anon_id, mut anon_rx) = join(&mgr).await;

    mgr.set_connection_user(auth_id, user("u", &["read"]))
        .await
        .unwrap();
    mgr.set_connection_user(admin_id, user("a", &["read", "admin"]))
        .await
        .unwrap();
    // anon stays unauthenticated.

    // broadcast_to_authenticated reaches both authenticated peers.
    let n = mgr
        .broadcast_to_authenticated(BidirectionalMessage::Ping)
        .await
        .unwrap();
    assert_eq!(n, 2);
    assert!(auth_rx.try_recv().is_ok());
    assert!(admin_rx.try_recv().is_ok());
    assert!(anon_rx.try_recv().is_err());

    // broadcast_to_permission only reaches the admin.
    let n = mgr
        .broadcast_to_permission("admin", BidirectionalMessage::Ping)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert!(admin_rx.try_recv().is_ok());

    // clear_connection_user flips the auth flag back.
    mgr.clear_connection_user(auth_id).await.unwrap();
    let n = mgr
        .broadcast_to_authenticated(BidirectionalMessage::Pong)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Setting/clearing user on missing id is best-effort, not an error.
    mgr.set_connection_user(ConnectionId::new(), user("ghost", &[]))
        .await
        .unwrap();
    mgr.clear_connection_user(ConnectionId::new())
        .await
        .unwrap();
}

#[tokio::test]
async fn broadcast_to_topic_counts_recipients_and_skips_empty() {
    let mgr = DefaultConnectionManager::new();
    let (a, mut ra) = join(&mgr).await;
    let (_b, _rb) = join(&mgr).await;
    mgr.add_subscription(a, "t".into()).await.unwrap();

    let n = mgr
        .broadcast_to_topic("t", BidirectionalMessage::Ping)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert!(ra.try_recv().is_ok());

    // Topic with no subscribers reports zero.
    let n = mgr
        .broadcast_to_topic("missing", BidirectionalMessage::Ping)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn pending_request_lifecycle() {
    let mgr = DefaultConnectionManager::new();
    let (id, _rx) = join(&mgr).await;

    let (tx, rx) = oneshot::channel();
    mgr.register_pending_request(id, serde_json::json!("rid"), tx)
        .await
        .unwrap();

    // remove_pending_request hands back the sender.
    let pulled = mgr
        .remove_pending_request(id, &serde_json::json!("rid"))
        .await
        .unwrap();
    assert!(pulled.is_some());
    drop(pulled);
    drop(rx);

    // handle_pending_response with no registered id reports false.
    let resp = JsonRpcResponse::success(serde_json::json!("ok"), Some(serde_json::json!("rid")));
    let handled = mgr.handle_pending_response(id, resp).await.unwrap();
    assert!(!handled);

    // Register again, then route a real response through handle_pending_response.
    let (tx, rx) = oneshot::channel();
    mgr.register_pending_request(id, serde_json::json!("rid2"), tx)
        .await
        .unwrap();
    let resp = JsonRpcResponse::success(serde_json::json!(7), Some(serde_json::json!("rid2")));
    assert!(mgr.handle_pending_response(id, resp).await.unwrap());
    let received = rx.await.unwrap();
    assert_eq!(received.result.unwrap(), serde_json::json!(7));

    // Removing for a connection that never registered any returns None.
    let pulled = mgr
        .remove_pending_request(ConnectionId::new(), &serde_json::json!("nope"))
        .await
        .unwrap();
    assert!(pulled.is_none());
}

#[tokio::test]
async fn send_to_missing_connection_is_silent_ok() {
    let mgr = DefaultConnectionManager::new();
    // Nothing registered — manager logs and returns Ok.
    mgr.send_to_connection(ConnectionId::new(), BidirectionalMessage::Ping)
        .await
        .unwrap();
}

#[tokio::test]
async fn default_impl_is_equivalent_to_new() {
    let _ = Arc::new(DefaultConnectionManager::default());
}
