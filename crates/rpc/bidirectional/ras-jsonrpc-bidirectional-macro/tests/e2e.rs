//! Socketless end-to-end tests for `jsonrpc_bidirectional_service!`.
//!
//! These tests avoid binding sockets by exercising the generated service handler
//! through the server message loop using an in-memory WebSocket adapter.

use std::collections::{HashSet, VecDeque};
use std::future;
use std::sync::Arc;

use async_trait::async_trait;
use ras_auth_core::AuthenticatedUser;
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use ras_jsonrpc_bidirectional_server::DefaultConnectionManager;
use ras_jsonrpc_bidirectional_server::connection::{ChannelMessageSender, ConnectionContext};
use ras_jsonrpc_bidirectional_server::handler::{
    WebSocketHandler, WebSocketIo, WebSocketIoMessage,
};
use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, ConnectionId, ConnectionInfo, ConnectionManager,
};
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EchoIn {
    pub msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EchoOut {
    pub msg: String,
    pub user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushNote {
    pub kind: String,
}

jsonrpc_bidirectional_service!({
    service_name: Demo,
    client_to_server: [
        UNAUTHORIZED hello(String) -> String,
        OPTIONAL_AUTH whoami(String) -> String,
        WITH_PERMISSIONS(["user"]) echo(EchoIn) -> EchoOut,
    ],
    server_to_client: [
        ping(PushNote),
    ],
    server_to_client_calls: [
    ]
});

#[derive(Clone)]
struct DemoImpl;

#[async_trait]
impl DemoService for DemoImpl {
    async fn hello(
        &self,
        _client: ConnectionId,
        _conns: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager,
        name: String,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok(format!("hello, {name}"))
    }

    async fn echo(
        &self,
        client: ConnectionId,
        conns: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager,
        user: &AuthenticatedUser,
        req: EchoIn,
    ) -> Result<EchoOut, Box<dyn std::error::Error + Send + Sync>> {
        // Also push a server→client notification so the test can observe it.
        let note = ras_jsonrpc_bidirectional_types::ServerNotification {
            method: "ping".to_string(),
            params: serde_json::to_value(PushNote {
                kind: "after-echo".into(),
            })
            .unwrap(),
            metadata: None,
        };
        let _ = conns
            .send_to_connection(
                client,
                ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(note),
            )
            .await;

        Ok(EchoOut {
            msg: req.msg,
            user: user.user_id.clone(),
        })
    }

    async fn whoami(
        &self,
        _client: ConnectionId,
        _conns: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager,
        caller: ras_auth_core::Caller,
        _req: String,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // OPTIONAL_AUTH: report the connection's user when present, anonymous otherwise.
        Ok(match caller {
            ras_auth_core::Caller::Authenticated(user) => user.user_id,
            ras_auth_core::Caller::Anonymous => "anonymous".to_string(),
        })
    }

    async fn notify_ping(
        &self,
        _connection_id: ConnectionId,
        _params: PushNote,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }
}

struct InMemorySocket {
    incoming: VecDeque<WebSocketIoMessage>,
    outgoing: Vec<WebSocketIoMessage>,
    close_when_empty: bool,
    close_after_outgoing: Option<usize>,
}

impl InMemorySocket {
    fn closing(incoming: impl IntoIterator<Item = WebSocketIoMessage>) -> Self {
        Self {
            incoming: incoming.into_iter().collect(),
            outgoing: Vec::new(),
            close_when_empty: true,
            close_after_outgoing: None,
        }
    }

    fn closing_after_outgoing(
        incoming: impl IntoIterator<Item = WebSocketIoMessage>,
        outgoing_count: usize,
    ) -> Self {
        Self {
            incoming: incoming.into_iter().collect(),
            outgoing: Vec::new(),
            close_when_empty: false,
            close_after_outgoing: Some(outgoing_count),
        }
    }
}

#[async_trait]
impl WebSocketIo for InMemorySocket {
    async fn send(
        &mut self,
        message: WebSocketIoMessage,
    ) -> ras_jsonrpc_bidirectional_server::ServerResult<()> {
        self.outgoing.push(message);
        if self
            .close_after_outgoing
            .is_some_and(|count| self.outgoing.len() >= count)
        {
            self.close_when_empty = true;
        }
        Ok(())
    }

    async fn recv(
        &mut self,
    ) -> Option<ras_jsonrpc_bidirectional_server::ServerResult<WebSocketIoMessage>> {
        if let Some(message) = self.incoming.pop_front() {
            Some(Ok(message))
        } else if self.close_when_empty {
            None
        } else {
            future::pending().await
        }
    }
}

async fn run_generated_handler(
    request: JsonRpcRequest,
    user: Option<AuthenticatedUser>,
    close_after_outgoing: Option<usize>,
) -> Vec<BidirectionalMessage> {
    run_generated_handler_with_provider(request, user, close_after_outgoing, None).await
}

/// Auth provider that treats the `*` permission as a grant of everything,
/// exercising custom `check_permissions` semantics (W5).
struct WildcardProvider;

impl ras_auth_core::AuthProvider for WildcardProvider {
    fn authenticate(&self, _token: String) -> ras_auth_core::AuthFuture<'_> {
        Box::pin(future::ready(Err(ras_auth_core::AuthError::InvalidToken)))
    }

    fn check_permissions(
        &self,
        user: &AuthenticatedUser,
        required: &[String],
    ) -> ras_auth_core::AuthResult<()> {
        if user.permissions.contains("*") {
            return Ok(());
        }
        let missing: Vec<String> = required
            .iter()
            .filter(|p| !user.permissions.contains(*p))
            .cloned()
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(ras_auth_core::AuthError::InsufficientPermissions {
                required: required.to_vec(),
                has: user.permissions.iter().cloned().collect(),
            })
        }
    }
}

async fn run_generated_handler_with_provider(
    request: JsonRpcRequest,
    user: Option<AuthenticatedUser>,
    close_after_outgoing: Option<usize>,
    auth_provider: Option<Arc<dyn ras_auth_core::AuthProvider>>,
) -> Vec<BidirectionalMessage> {
    let connection_manager = Arc::new(DefaultConnectionManager::new());
    let mut handler = DemoHandler::new(Arc::new(DemoImpl), connection_manager.clone());
    if let Some(provider) = auth_provider {
        handler = handler.with_auth_provider(provider);
    }
    let handler = Arc::new(handler);

    let connection_id = ConnectionId::new();
    let (message_tx, message_rx) = mpsc::channel(8);
    let sender = ChannelMessageSender::new(connection_id, message_tx);

    let mut info = ConnectionInfo::new(connection_id);
    let context = Arc::new(ConnectionContext::new(connection_id, sender.clone()));
    if let Some(user) = user {
        info.set_user(user.clone());
        context.set_user(user).await;
    }

    connection_manager
        .add_connection_with_sender(info, Box::new(sender))
        .await
        .expect("connection should register");

    let request_text = serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap();
    let incoming = [WebSocketIoMessage::Text(request_text)];
    let mut socket = if let Some(count) = close_after_outgoing {
        InMemorySocket::closing_after_outgoing(incoming, count)
    } else {
        InMemorySocket::closing(incoming)
    };

    WebSocketHandler::new(handler, context, message_rx, 4096)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    socket
        .outgoing
        .into_iter()
        .filter_map(|message| match message {
            WebSocketIoMessage::Text(text) => serde_json::from_str(&text).ok(),
            _ => None,
        })
        .collect()
}

fn test_user(user_id: &str, permissions: &[&str]) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: user_id.to_string(),
        permissions: permissions
            .iter()
            .map(|permission| (*permission).to_string())
            .collect::<HashSet<_>>(),
        metadata: None,
    }
}

fn response_from(messages: &[BidirectionalMessage]) -> &JsonRpcResponse {
    messages
        .iter()
        .find_map(|message| match message {
            BidirectionalMessage::Response(response) => Some(response),
            _ => None,
        })
        .expect("response should be sent")
}

#[tokio::test]
async fn generated_handler_round_trips_without_socket() {
    let messages = run_generated_handler(
        JsonRpcRequest::new(
            "hello".into(),
            Some(serde_json::json!("alice")),
            Some(1.into()),
        ),
        None,
        None,
    )
    .await;

    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));

    let response = response_from(&messages);
    assert!(response.error.is_none());
    assert_eq!(response.result, Some(serde_json::json!("hello, alice")));

    assert!(matches!(
        messages.last().unwrap(),
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}

#[tokio::test]
async fn optional_auth_handler_is_anonymous_without_user() {
    let messages = run_generated_handler(
        JsonRpcRequest::new(
            "whoami".into(),
            Some(serde_json::json!("ignored")),
            Some(5.into()),
        ),
        None,
        None,
    )
    .await;

    let response = response_from(&messages);
    assert!(response.error.is_none());
    assert_eq!(response.result, Some(serde_json::json!("anonymous")));
}

#[tokio::test]
async fn optional_auth_handler_identifies_connection_user() {
    let messages = run_generated_handler(
        JsonRpcRequest::new(
            "whoami".into(),
            Some(serde_json::json!("ignored")),
            Some(6.into()),
        ),
        Some(test_user("user-1", &[])),
        None,
    )
    .await;

    let response = response_from(&messages);
    assert!(response.error.is_none());
    assert_eq!(response.result, Some(serde_json::json!("user-1")));
}

#[tokio::test]
async fn w5_generated_handler_routes_permission_check_through_provider() {
    let request = JsonRpcRequest::new(
        "echo".into(),
        Some(serde_json::json!(EchoIn { msg: "hi".into() })),
        Some(3.into()),
    );

    // Plain set membership: "*" is not the literal "user" permission.
    let messages =
        run_generated_handler(request.clone(), Some(test_user("root", &["*"])), None).await;
    assert!(response_from(&messages).error.is_some());

    // With the provider attached, its wildcard semantics apply over WebSocket.
    let messages = run_generated_handler_with_provider(
        request,
        Some(test_user("root", &["*"])),
        None,
        Some(Arc::new(WildcardProvider)),
    )
    .await;
    let response = response_from(&messages);
    assert!(response.error.is_none(), "provider grant must be honored");

    // The error payload never echoes the caller's own grant set.
    let messages = run_generated_handler_with_provider(
        JsonRpcRequest::new(
            "echo".into(),
            Some(serde_json::json!(EchoIn { msg: "hi".into() })),
            Some(4.into()),
        ),
        Some(test_user("readonly", &["read"])),
        None,
        Some(Arc::new(WildcardProvider)),
    )
    .await;
    let error = response_from(&messages).error.as_ref().unwrap();
    assert_eq!(error.code, -32002);
    let data = error.data.as_ref().unwrap().to_string();
    assert!(data.contains("\"user\""));
    assert!(!data.contains("\"read\""));
}

#[tokio::test]
async fn generated_handler_enforces_permissions_without_socket() {
    let messages = run_generated_handler(
        JsonRpcRequest::new(
            "echo".into(),
            Some(serde_json::json!(EchoIn { msg: "hi".into() })),
            Some(2.into()),
        ),
        Some(test_user("readonly", &["read"])),
        None,
    )
    .await;

    let response = response_from(&messages);
    let error = response.error.as_ref().expect("permission error expected");
    assert_eq!(error.code, -32002);
}

#[tokio::test]
async fn generated_handler_sends_response_and_notification_without_socket() {
    let messages = run_generated_handler(
        JsonRpcRequest::new(
            "echo".into(),
            Some(serde_json::json!(EchoIn { msg: "hi".into() })),
            Some(3.into()),
        ),
        Some(test_user("user-1", &["user"])),
        Some(3),
    )
    .await;

    let response = response_from(&messages);
    let result: EchoOut = serde_json::from_value(response.result.clone().unwrap()).unwrap();
    assert_eq!(result.msg, "hi");
    assert_eq!(result.user, "user-1");

    let notification = messages
        .iter()
        .find_map(|message| match message {
            BidirectionalMessage::ServerNotification(notification) => Some(notification),
            _ => None,
        })
        .expect("server notification should be sent");
    assert_eq!(notification.method, "ping");
    assert_eq!(notification.params["kind"], "after-echo");
}
