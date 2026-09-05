use super::*;

#[tokio::test(start_paused = true)]
async fn w4_idle_connection_is_pinged_then_closed() {
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::pending();

    WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
        .with_keepalive(KeepaliveConfig {
            ping_interval: Some(Duration::from_secs(5)),
            idle_timeout: Some(Duration::from_secs(12)),
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let pings = socket
        .outgoing
        .iter()
        .filter(|m| matches!(m, WebSocketIoMessage::Ping(_)))
        .count();
    assert_eq!(pings, 2, "pings at 5s and 10s before the 12s idle close");
    assert!(socket.outgoing.iter().any(|m| matches!(
        m,
        WebSocketIoMessage::Close(Some(reason)) if reason == "idle timeout"
    )));
}

#[tokio::test(start_paused = true)]
async fn w4_zero_durations_do_not_panic() {
    let (_tx, rx) = mpsc::channel(4);
    let mut socket = InMemorySocket::pending();

    // Zero ping and idle: both disabled; zero revalidation: default used.
    // The sequence provider fails on its first tick, which closes the loop.
    WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
        .with_keepalive(KeepaliveConfig {
            ping_interval: Some(Duration::ZERO),
            idle_timeout: Some(Duration::ZERO),
        })
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(SequenceAuthProvider::new([])),
            token: "t".into(),
            interval: Duration::ZERO,
            on_permission_change: PermissionChangePolicy::default(),
        })
        .run_with_io(&mut socket)
        .await
        .unwrap();

    assert!(
        !socket
            .outgoing
            .iter()
            .any(|m| matches!(m, WebSocketIoMessage::Ping(_)))
    );
    assert!(socket.outgoing.iter().any(|m| matches!(
        m,
        WebSocketIoMessage::Close(Some(reason)) if reason == "credentials no longer valid"
    )));
}
