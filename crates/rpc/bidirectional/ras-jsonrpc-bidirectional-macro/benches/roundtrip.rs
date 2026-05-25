//! Criterion bench measuring generated service dispatch through the in-memory
//! WebSocket message loop.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
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
use ras_jsonrpc_types::JsonRpcRequest;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EchoIn {
    msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EchoOut {
    msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ignored;

fn mock_user(user_id: &str, permissions: &[&str]) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: user_id.to_string(),
        permissions: permissions
            .iter()
            .map(|p| (*p).to_string())
            .collect::<HashSet<_>>(),
        metadata: None,
    }
}

jsonrpc_bidirectional_service!({
    service_name: BenchSvc,
    client_to_server: [
        WITH_PERMISSIONS(["user"]) echo(EchoIn) -> EchoOut,
    ],
    server_to_client: [
        unused(Ignored),
    ],
    server_to_client_calls: [
    ]
});

#[derive(Clone)]
struct BenchImpl;

#[async_trait]
impl BenchSvcService for BenchImpl {
    async fn echo(
        &self,
        _client: ConnectionId,
        _conns: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager,
        _user: &AuthenticatedUser,
        req: EchoIn,
    ) -> Result<EchoOut, Box<dyn std::error::Error + Send + Sync>> {
        Ok(EchoOut { msg: req.msg })
    }

    async fn notify_unused(
        &self,
        _connection_id: ConnectionId,
        _params: Ignored,
    ) -> ras_jsonrpc_bidirectional_types::Result<()> {
        Ok(())
    }
}

struct InMemorySocket {
    incoming: VecDeque<WebSocketIoMessage>,
    outgoing: Vec<WebSocketIoMessage>,
}

impl InMemorySocket {
    fn closing(incoming: impl IntoIterator<Item = WebSocketIoMessage>) -> Self {
        Self {
            incoming: incoming.into_iter().collect(),
            outgoing: Vec::new(),
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
        Ok(())
    }

    async fn recv(
        &mut self,
    ) -> Option<ras_jsonrpc_bidirectional_server::ServerResult<WebSocketIoMessage>> {
        self.incoming.pop_front().map(Ok)
    }
}

async fn run_in_memory_roundtrip() -> EchoOut {
    let cm = Arc::new(DefaultConnectionManager::new());
    let handler = Arc::new(BenchSvcHandler::new(Arc::new(BenchImpl), cm.clone()));

    let connection_id = ConnectionId::new();
    let (message_tx, message_rx) = mpsc::channel(8);
    let sender = ChannelMessageSender::new(connection_id, message_tx);
    let user = mock_user("user-1", &["user"]);

    let mut info = ConnectionInfo::new(connection_id);
    info.set_user(user.clone());
    let context = Arc::new(ConnectionContext::new(connection_id, sender.clone()));
    context.set_user(user).await;
    cm.add_connection_with_sender(info, Box::new(sender))
        .await
        .expect("benchmark connection should register");

    let request = JsonRpcRequest::new(
        "echo".to_string(),
        Some(serde_json::json!(EchoIn { msg: "x".into() })),
        Some(1.into()),
    );
    let request_text = serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap();
    let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text(request_text)]);

    WebSocketHandler::new(handler, context, message_rx, 4096)
        .run_with_io(&mut socket)
        .await
        .expect("in-memory benchmark roundtrip should complete");

    socket
        .outgoing
        .into_iter()
        .filter_map(|message| match message {
            WebSocketIoMessage::Text(text) => {
                serde_json::from_str::<BidirectionalMessage>(&text).ok()
            }
            _ => None,
        })
        .find_map(|message| match message {
            BidirectionalMessage::Response(response) => response
                .result
                .map(|result| serde_json::from_value(result).unwrap()),
            _ => None,
        })
        .expect("benchmark response should be sent")
}

fn bench_roundtrip(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("in_memory_ws_echo_roundtrip", |b| {
        b.to_async(&rt).iter(|| async {
            let r = run_in_memory_roundtrip().await;
            std::hint::black_box(r);
        });
    });
}

criterion_group!(benches, bench_roundtrip);
criterion_main!(benches);
