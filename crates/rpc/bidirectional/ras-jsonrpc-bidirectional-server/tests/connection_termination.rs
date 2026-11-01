//! The server can always end a connection: forced disconnects and revocation take effect even while the socket is stalled.

use async_trait::async_trait;
use axum::{Router, routing::get};
use futures::{SinkExt, StreamExt};
use ras_auth_core::{AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_bidirectional_server::{
    AuthRevalidation, ChannelMessageSender, ConnectionContext, ConnectionId,
    DefaultConnectionManager, KeepaliveConfig, MessageHandler, MessageRouter,
    PermissionChangePolicy, ServerResult, WebSocketHandler, WebSocketService,
    WebSocketServiceBuilder,
    handler::{WebSocketIo, WebSocketIoMessage},
    service::{BuiltWebSocketService, websocket_handler},
};
use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, ConnectionManager, manager::ConnectionManagerExt,
};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};

fn user() -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "victim".into(),
        permissions: HashSet::from(["private:read".into()]),
        metadata: None,
    }
}

struct Allow;
impl AuthProvider for Allow {
    fn authenticate(&self, _: String) -> AuthFuture<'_> {
        Box::pin(async { Ok(user()) })
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Serve a router with one authenticated method and connect to it.
async fn serve() -> (
    Arc<DefaultConnectionManager>,
    Socket,
    tokio::task::JoinHandle<()>,
) {
    let mut router = MessageRouter::new();
    router.register_value("private.read", |_, context| async move {
        Ok::<_, ras_jsonrpc_bidirectional_server::ServerError>(
            context.has_permission("private:read").await,
        )
    });
    let service = WebSocketServiceBuilder::builder()
        .handler(Arc::new(router))
        .auth_provider(Arc::new(Allow))
        .build()
        .build();
    let manager = service.connection_manager();
    type Service = BuiltWebSocketService<MessageRouter, Allow, DefaultConnectionManager>;
    let app = Router::new()
        .route("/ws", get(websocket_handler::<Service>))
        .with_state(service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", "Bearer review-token".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let _hello = ws.next().await.unwrap().unwrap();
    (manager, ws, server)
}

/// Read until the server closes, within one second. Returns the parsed text
/// frames and the close reason.
async fn read_until_closed(ws: &mut Socket) -> (Vec<BidirectionalMessage>, Option<String>) {
    tokio::time::timeout(Duration::from_secs(1), async {
        let mut messages = Vec::new();
        let mut close_reason = None;
        while let Some(Ok(frame)) = ws.next().await {
            match frame {
                Message::Text(text) => {
                    if let Ok(message) = serde_json::from_str(&text) {
                        messages.push(message);
                    }
                }
                Message::Close(frame) => close_reason = frame.map(|f| f.reason.to_string()),
                _ => {}
            }
        }
        (messages, close_reason)
    })
    .await
    .expect("forced disconnect must promptly close the socket")
}

#[tokio::test]
async fn forced_disconnect_must_stop_authenticated_requests() {
    let (manager, mut ws, server) = serve().await;
    assert_eq!(manager.disconnect_user("victim").await.unwrap(), 1);
    assert_eq!(manager.connection_count(), 0);
    // The server may already have closed the socket; a failed send is fine.
    let _ = ws
        .send(Message::Text(
            serde_json::to_string(&JsonRpcRequest::new(
                "private.read".into(),
                None,
                Some(serde_json::json!(1)),
            ))
            .unwrap()
            .into(),
        ))
        .await;
    let (messages, close_reason) = read_until_closed(&mut ws).await;
    server.abort();
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, BidirectionalMessage::Response(_))),
        "forced-disconnected user still executes an authenticated operation"
    );
    assert_eq!(close_reason.as_deref(), Some("disconnected by server"));
}

#[tokio::test]
async fn forced_disconnect_delivers_messages_queued_before_it() {
    let (manager, mut ws, server) = serve().await;
    let id = manager.get_connection_ids()[0];
    manager
        .notify_connection(id, "kicked", serde_json::json!({"reason": "spam"}))
        .await
        .unwrap();
    manager.remove_connection(id).await.unwrap();

    let (messages, close_reason) = read_until_closed(&mut ws).await;
    server.abort();
    assert!(messages.iter().any(|message| matches!(
        message,
        BidirectionalMessage::ServerNotification(notification) if notification.method == "kicked"
    )));
    assert_eq!(close_reason.as_deref(), Some("disconnected by server"));
}

struct CountingDeny(Arc<AtomicUsize>);
impl AuthProvider for CountingDeny {
    fn authenticate(&self, _: String) -> AuthFuture<'_> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(ras_auth_core::AuthError::InvalidToken) })
    }
}

struct NoRequests;
#[async_trait]
impl MessageHandler for NoRequests {
    async fn handle_request(
        &self,
        _: JsonRpcRequest,
        _: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        Ok(None)
    }
}

struct BlockSecondWrite(usize);
#[async_trait]
impl WebSocketIo for BlockSecondWrite {
    async fn send(&mut self, _: WebSocketIoMessage) -> ServerResult<()> {
        self.0 += 1;
        if self.0 >= 2 {
            std::future::pending().await
        } else {
            Ok(())
        }
    }
    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn blocked_socket_write_must_not_suspend_revocation_checks() {
    let id = ConnectionId::new();
    let (tx, rx) = mpsc::channel(1);
    let sender = ChannelMessageSender::new(id, tx);
    sender.send(BidirectionalMessage::Ping).await.unwrap();
    let context = Arc::new(ConnectionContext::new(id, sender));
    context.set_user(user()).await;
    let checks = Arc::new(AtomicUsize::new(0));
    let handler = WebSocketHandler::new(Arc::new(NoRequests), context, rx, 1024)
        .with_keepalive(KeepaliveConfig {
            ping_interval: None,
            idle_timeout: Some(Duration::from_millis(20)),
        })
        .with_auth_revalidation(AuthRevalidation {
            auth_provider: Arc::new(CountingDeny(checks.clone())),
            token: "revoked".into(),
            interval: Duration::from_millis(10),
            on_permission_change: PermissionChangePolicy::Close,
        });
    let mut socket = BlockSecondWrite(0);
    let finished =
        tokio::time::timeout(Duration::from_millis(100), handler.run_with_io(&mut socket)).await;
    assert!(
        checks.load(Ordering::SeqCst) > 0,
        "socket backpressure suppressed every revocation check and the idle timeout"
    );
    assert!(finished.is_ok(), "the revoked connection must end");
}

#[tokio::test]
async fn forced_disconnect_ends_a_connection_stalled_on_write() {
    let id = ConnectionId::new();
    let (tx, rx) = mpsc::channel(1);
    let sender = ChannelMessageSender::new(id, tx);
    sender.send(BidirectionalMessage::Ping).await.unwrap();
    let context = Arc::new(ConnectionContext::new(id, sender.clone()));
    // No re-validation and no idle timeout: only the close signal can end it.
    let handler = WebSocketHandler::new(Arc::new(NoRequests), context, rx, 1024).with_keepalive(
        KeepaliveConfig {
            ping_interval: None,
            idle_timeout: None,
        },
    );
    let mut socket = BlockSecondWrite(0);
    let kick = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        sender.close();
    };
    let (finished, ()) = tokio::join!(
        tokio::time::timeout(Duration::from_millis(500), handler.run_with_io(&mut socket)),
        kick
    );
    assert!(
        finished.is_ok(),
        "a stalled write must not keep a disconnected connection alive"
    );
}
