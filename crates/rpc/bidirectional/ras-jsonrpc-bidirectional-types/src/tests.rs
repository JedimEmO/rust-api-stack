use super::*;
use uuid::Uuid;

#[test]
fn test_connection_id() {
    let id1 = ConnectionId::new();
    let id2 = ConnectionId::new();
    assert_ne!(id1, id2);

    let uuid = Uuid::new_v4();
    let id3 = ConnectionId::from_uuid(uuid);
    assert_eq!(id3.as_uuid(), &uuid);
}

#[test]
fn test_connection_info() {
    let mut info = ConnectionInfo::new(ConnectionId::new());
    assert!(!info.is_authenticated());
    assert!(!info.has_permission("admin"));

    // Test subscriptions
    info.subscribe("topic1".to_string());
    info.subscribe("topic2".to_string());
    assert!(info.is_subscribed_to("topic1"));
    assert!(info.is_subscribed_to("topic2"));
    assert!(!info.is_subscribed_to("topic3"));

    assert!(info.unsubscribe("topic1"));
    assert!(!info.is_subscribed_to("topic1"));
    assert!(!info.unsubscribe("topic1")); // Already unsubscribed
}

#[test]
fn test_message_serialization() {
    let msg = BidirectionalMessage::Ping;
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"ping\""));

    let notification = ServerNotification {
        method: "test.notify".to_string(),
        params: serde_json::json!({"data": "test"}),
        metadata: None,
    };
    let msg = BidirectionalMessage::ServerNotification(notification);
    let json = serde_json::to_string(&msg).unwrap();
    let deserialized: BidirectionalMessage = serde_json::from_str(&json).unwrap();

    if let BidirectionalMessage::ServerNotification(notif) = deserialized {
        assert_eq!(notif.method, "test.notify");
    } else {
        panic!("Expected ServerNotification");
    }
}
