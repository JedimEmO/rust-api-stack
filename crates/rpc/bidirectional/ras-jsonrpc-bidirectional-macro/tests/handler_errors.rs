//! Over WebSocket, as over HTTP, a handler's error reaches the client only when
//! it returns a `PublicRpcError` directly, with a code it is allowed to use.

use std::collections::{HashSet, VecDeque};
use std::future;
use std::sync::Arc;

use async_trait::async_trait;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser, Caller};
use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use ras_jsonrpc_bidirectional_server::DefaultConnectionManager;
use ras_jsonrpc_bidirectional_server::connection::{ChannelMessageSender, ConnectionContext};
use ras_jsonrpc_bidirectional_server::handler::{
    WebSocketHandler, WebSocketIo, WebSocketIoMessage,
};
use ras_jsonrpc_bidirectional_types::{
    BidirectionalMessage, ConnectionId, ConnectionInfo, ConnectionManager,
};
use ras_jsonrpc_types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, PublicRpcError};
use serde_json::{Value, json};
use tokio::sync::mpsc;

jsonrpc_bidirectional_service!({
    service_name: Errors,
    client_to_server: [
        UNAUTHORIZED fail(String) -> String,
        OPTIONAL_AUTH fail_optional(String) -> String,
        WITH_PERMISSIONS(["user"]) fail_user(String) -> String,
    ],
    server_to_client: [
    ],
    server_to_client_calls: [
    ]
});

#[derive(Debug, thiserror::Error)]
#[error("upstream failed")]
struct Upstream(#[source] PublicRpcError);

type HandlerResult = Result<String, Box<dyn std::error::Error + Send + Sync>>;

/// The outcome every method produces for a given `kind`.
fn outcome(kind: String) -> HandlerResult {
    match kind.as_str() {
        "public" => Err(PublicRpcError::new(1001, "Email already registered")
            .with_data(json!({ "field": "email" }))
            .into()),
        "reserved" => Err(PublicRpcError::new(-32001, "secret fake auth failure").into()),
        "plain" => Err(std::io::Error::other("password=hunter2").into()),
        "wrapped" => Err(Upstream(PublicRpcError::new(1001, "secret detail")).into()),
        _ => Ok(kind),
    }
}

#[derive(Clone)]
struct ErrorsImpl;

#[async_trait]
impl ErrorsService for ErrorsImpl {
    async fn fail(
        &self,
        _client: ConnectionId,
        _conns: &dyn ConnectionManager,
        kind: String,
    ) -> HandlerResult {
        outcome(kind)
    }

    async fn fail_optional(
        &self,
        _client: ConnectionId,
        _conns: &dyn ConnectionManager,
        _caller: Caller,
        kind: String,
    ) -> HandlerResult {
        outcome(kind)
    }

    async fn fail_user(
        &self,
        _client: ConnectionId,
        _conns: &dyn ConnectionManager,
        _user: &AuthenticatedUser,
        kind: String,
    ) -> HandlerResult {
        outcome(kind)
    }
}

struct InMemorySocket {
    incoming: VecDeque<WebSocketIoMessage>,
    outgoing: Vec<WebSocketIoMessage>,
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

fn user() -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: "u1".into(),
        permissions: HashSet::from(["user".to_string()]),
        metadata: None,
    }
}

/// Runs one request through the generated handler and the server's message
/// loop, and returns the response it sent.
async fn respond(method: &str, kind: &str) -> JsonRpcResponse {
    responses(method, kind, Some(json!(1)))
        .await
        .into_iter()
        .next()
        .expect("a response was sent")
}

/// Runs one request, or a notification when `id` is `None`, and returns every
/// response the server sent for it.
async fn responses(method: &str, kind: &str, id: Option<Value>) -> Vec<JsonRpcResponse> {
    let connection_manager = Arc::new(DefaultConnectionManager::new());
    let handler = Arc::new(ErrorsHandler::new(
        Arc::new(ErrorsImpl),
        connection_manager.clone(),
    ));

    let connection_id = ConnectionId::new();
    let (message_tx, message_rx) = mpsc::channel(8);
    let sender = ChannelMessageSender::new(connection_id, message_tx);
    let mut info = ConnectionInfo::new(connection_id);
    info.set_user(user());
    let context = Arc::new(ConnectionContext::new(connection_id, sender.clone()));
    context.set_user(user()).await;
    connection_manager
        .add_connection_with_sender(info, Box::new(sender))
        .await
        .expect("connection registers");

    let request = JsonRpcRequest::new(method.into(), Some(json!(kind)), id);
    let text = serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap();
    let mut socket = InMemorySocket {
        incoming: VecDeque::from([WebSocketIoMessage::Text(text)]),
        outgoing: Vec::new(),
    };
    WebSocketHandler::new(handler, context, message_rx, 4096)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    socket
        .outgoing
        .into_iter()
        .filter_map(|message| match message {
            WebSocketIoMessage::Text(text) => match serde_json::from_str(&text).ok()? {
                BidirectionalMessage::Response(response) => Some(response),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn notifications_get_no_response() {
    for method in METHODS {
        for kind in ["ok", "public", "plain"] {
            let sent = responses(method, kind, None).await;
            assert!(sent.is_empty(), "{method} {kind}: {sent:?}");
        }
    }
}

fn assert_hidden(label: &str, response: &JsonRpcResponse) {
    let error = response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("{label}: no error"));
    assert_eq!(error.code, -32603, "{label}: {error:?}");
    assert_eq!(error.message, "Internal error", "{label}: {error:?}");
    assert_eq!(error.data, None, "{label}");
    let text = serde_json::to_string(response).unwrap();
    assert!(!text.contains("secret"), "{label} leaked: {text}");
    assert!(!text.contains("hunter2"), "{label} leaked: {text}");
}

const METHODS: [&str; 3] = ["fail", "fail_optional", "fail_user"];

#[tokio::test]
async fn a_returned_public_error_is_sent_from_every_auth_branch() {
    for method in METHODS {
        let response = respond(method, "public").await;
        assert_eq!(
            response.error,
            Some(JsonRpcError::new(
                1001,
                "Email already registered".into(),
                Some(json!({ "field": "email" }))
            )),
            "{method}"
        );
    }
}

#[tokio::test]
async fn reserved_codes_and_other_errors_stay_hidden() {
    for method in METHODS {
        for kind in ["reserved", "plain", "wrapped"] {
            assert_hidden(&format!("{method} {kind}"), &respond(method, kind).await);
        }
    }
}

struct RejectAll;

impl AuthProvider for RejectAll {
    fn authenticate(&self, _token: String) -> AuthFuture<'_> {
        Box::pin(future::ready(Err(AuthError::InvalidToken)))
    }
}

/// The generated client over a real WebSocket returns the error as
/// `ClientError::Rpc`, with code, message and data.
#[tokio::test]
async fn the_generated_client_receives_the_public_error() {
    type Service = ras_jsonrpc_bidirectional_server::service::BuiltWebSocketService<
        ErrorsHandler<ErrorsImpl, DefaultConnectionManager>,
        RejectAll,
        DefaultConnectionManager,
    >;
    let service = ErrorsBuilder::new(ErrorsImpl, RejectAll)
        .require_auth(false)
        .build();
    let app = axum::Router::new()
        .route(
            "/ws",
            axum::routing::get(
                ras_jsonrpc_bidirectional_server::service::websocket_handler::<Service>,
            ),
        )
        .with_state(service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });

    let client = ErrorsClientBuilder::new(format!("ws://{address}/ws"))
        .build()
        .await
        .expect("client builds");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        client.connect().await.expect("connects");

        let error = client
            .fail("public".into())
            .await
            .expect_err("handler fails");
        match error {
            ras_jsonrpc_bidirectional_client::ClientError::Rpc(error) => {
                assert_eq!(error.code, 1001);
                assert_eq!(error.message, "Email already registered");
                assert_eq!(error.data, Some(json!({ "field": "email" })));
            }
            other => panic!("expected ClientError::Rpc, got {other:?}"),
        }

        let hidden = client
            .fail("plain".into())
            .await
            .expect_err("handler fails");
        match hidden {
            ras_jsonrpc_bidirectional_client::ClientError::Rpc(error) => {
                assert_eq!(error.code, -32603);
                assert_eq!(error.data, None::<Value>);
            }
            other => panic!("expected ClientError::Rpc, got {other:?}"),
        }
    })
    .await
    .expect("finishes in time");
}
