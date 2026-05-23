//! Message handlers for WebSocket communication

use crate::{ConnectionContext, ServerError, ServerResult};
use async_trait::async_trait;
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::stream::StreamExt;
use ras_jsonrpc_bidirectional_types::BidirectionalMessage;
use ras_jsonrpc_types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, error_codes};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Trait for handling JSON-RPC requests within a WebSocket context
#[async_trait]
pub trait MessageHandler: Send + Sync + 'static {
    /// Handle an incoming JSON-RPC request
    ///
    /// # Arguments
    /// * `request` - The JSON-RPC request to handle
    /// * `context` - The connection context containing auth info and metadata
    ///
    /// # Returns
    /// * `Ok(Some(response))` - Response to send back to client
    /// * `Ok(None)` - No response needed (for notifications)
    /// * `Err(error)` - Error occurred during handling
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>>;

    /// Handle subscription requests
    async fn handle_subscribe(
        &self,
        topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        // Default implementation subscribes the connection to each requested topic.
        for topic in topics {
            context.subscribe(topic).await;
        }
        Ok(())
    }

    /// Handle unsubscription requests
    async fn handle_unsubscribe(
        &self,
        topics: Vec<String>,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<()> {
        // Default implementation unsubscribes the connection from each requested topic.
        for topic in topics {
            context.unsubscribe(&topic).await;
        }
        Ok(())
    }

    /// Handle connection established event
    async fn on_connect(&self, context: Arc<ConnectionContext>) -> ServerResult<()> {
        info!("Connection established: {}", context.id);
        Ok(())
    }

    /// Handle connection closed event
    async fn on_disconnect(
        &self,
        context: Arc<ConnectionContext>,
        reason: Option<String>,
    ) -> ServerResult<()> {
        info!("Connection closed: {} (reason: {:?})", context.id, reason);
        Ok(())
    }

    /// Handle ping message
    async fn on_ping(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        // Default implementation records the ping at debug level.
        debug!("Received ping");
        Ok(())
    }

    /// Handle pong message
    async fn on_pong(&self, _context: Arc<ConnectionContext>) -> ServerResult<()> {
        // Default implementation records the pong at debug level.
        debug!("Received pong");
        Ok(())
    }
}

/// WebSocket message shape used by the server handler loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketIoMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<String>),
}

impl From<Message> for WebSocketIoMessage {
    fn from(message: Message) -> Self {
        match message {
            Message::Text(text) => Self::Text(text.to_string()),
            Message::Binary(data) => Self::Binary(data.to_vec()),
            Message::Ping(data) => Self::Ping(data.to_vec()),
            Message::Pong(data) => Self::Pong(data.to_vec()),
            Message::Close(frame) => Self::Close(frame.map(|frame| frame.reason.to_string())),
        }
    }
}

/// Minimal socket interface used by the message loop.
#[async_trait]
pub trait WebSocketIo: Send {
    async fn send(&mut self, message: WebSocketIoMessage) -> ServerResult<()>;
    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>>;
}

pub(crate) struct AxumWebSocketIo {
    socket: WebSocket,
}

impl AxumWebSocketIo {
    pub(crate) fn new(socket: WebSocket) -> Self {
        Self { socket }
    }
}

#[async_trait]
impl WebSocketIo for AxumWebSocketIo {
    async fn send(&mut self, message: WebSocketIoMessage) -> ServerResult<()> {
        let message = match message {
            WebSocketIoMessage::Text(text) => Message::Text(text.into()),
            WebSocketIoMessage::Binary(data) => Message::Binary(data.into()),
            WebSocketIoMessage::Ping(data) => Message::Ping(data.into()),
            WebSocketIoMessage::Pong(data) => Message::Pong(data.into()),
            WebSocketIoMessage::Close(reason) => Message::Close(reason.map(|reason| CloseFrame {
                code: axum::extract::ws::close_code::NORMAL,
                reason: reason.into(),
            })),
        };

        self.socket
            .send(message)
            .await
            .map_err(|e| ServerError::WebSocketError(e.to_string()))
    }

    async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
        self.socket.next().await.map(|message| {
            message
                .map(WebSocketIoMessage::from)
                .map_err(|e| ServerError::WebSocketError(e.to_string()))
        })
    }
}

/// WebSocket connection handler that manages the message flow
pub struct WebSocketHandler<H: MessageHandler> {
    /// The message handler for processing requests
    handler: Arc<H>,
    /// Connection context
    context: Arc<ConnectionContext>,
    /// Channel for receiving messages to send to client
    message_rx: mpsc::Receiver<BidirectionalMessage>,
    max_message_size: usize,
}

impl<H: MessageHandler> WebSocketHandler<H> {
    /// Create a new WebSocket handler
    pub fn new(
        handler: Arc<H>,
        context: Arc<ConnectionContext>,
        message_rx: mpsc::Receiver<BidirectionalMessage>,
        max_message_size: usize,
    ) -> Self {
        Self {
            handler,
            context,
            message_rx,
            max_message_size,
        }
    }

    /// Run the WebSocket handler loop
    pub async fn run(self, socket: WebSocket) -> ServerResult<()> {
        let mut socket = AxumWebSocketIo::new(socket);
        self.run_with_io(&mut socket).await
    }

    /// Run the handler loop over an already-upgraded socket implementation.
    pub async fn run_with_io<S: WebSocketIo + ?Sized>(
        mut self,
        socket: &mut S,
    ) -> ServerResult<()> {
        info!(
            "Starting WebSocket handler for connection: {}",
            self.context.id
        );

        // Notify handler of connection
        if let Err(e) = self.handler.on_connect(self.context.clone()).await {
            error!("Error in on_connect handler: {}", e);
        }

        // Send connection established message
        let established_msg = BidirectionalMessage::ConnectionEstablished {
            connection_id: self.context.id,
        };
        if let Err(e) = socket
            .send(WebSocketIoMessage::Text(serde_json::to_string(
                &established_msg,
            )?))
            .await
        {
            error!("Failed to send connection established message: {}", e);
        }

        // Main message handling loop
        loop {
            tokio::select! {
                // Handle incoming WebSocket messages
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(msg)) => {
                            if let Err(e) = self.handle_websocket_message(msg, socket).await {
                                error!("Error handling WebSocket message: {}", e);
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        None => {
                            debug!("WebSocket connection closed by client");
                            break;
                        }
                    }
                }

                // Handle outgoing messages
                msg = self.message_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if let Err(e) = self.send_message(socket, msg).await {
                                error!("Error sending message: {}", e);
                                break;
                            }
                        }
                        None => {
                            debug!("Message channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Notify handler of disconnection
        if let Err(e) = self.handler.on_disconnect(self.context.clone(), None).await {
            error!("Error in on_disconnect handler: {}", e);
        }

        // Send connection closed message
        let closed_msg = BidirectionalMessage::ConnectionClosed {
            connection_id: self.context.id,
            reason: None,
        };
        let _ = socket
            .send(WebSocketIoMessage::Text(serde_json::to_string(
                &closed_msg,
            )?))
            .await;

        info!(
            "WebSocket handler finished for connection: {}",
            self.context.id
        );
        Ok(())
    }

    /// Handle incoming WebSocket messages
    async fn handle_websocket_message<S: WebSocketIo + ?Sized>(
        &mut self,
        msg: WebSocketIoMessage,
        socket: &mut S,
    ) -> ServerResult<()> {
        match msg {
            WebSocketIoMessage::Text(text) => {
                if text.len() > self.max_message_size {
                    warn!("Received oversized text message: {} bytes", text.len());
                    return Err(ServerError::InvalidRequest(
                        "Message exceeds maximum size".to_string(),
                    ));
                }
                debug!("Received text message ({} bytes)", text.len());
                self.handle_text_message(text, socket).await
            }
            WebSocketIoMessage::Binary(data) => {
                if data.len() > self.max_message_size {
                    warn!("Received oversized binary message: {} bytes", data.len());
                    return Err(ServerError::InvalidRequest(
                        "Message exceeds maximum size".to_string(),
                    ));
                }
                debug!("Received binary message ({} bytes)", data.len());
                // Try to parse as UTF-8 text
                match String::from_utf8(data) {
                    Ok(text) => self.handle_text_message(text, socket).await,
                    Err(_) => {
                        warn!("Received non-UTF-8 binary message, ignoring");
                        Ok(())
                    }
                }
            }
            WebSocketIoMessage::Ping(data) => {
                debug!("Received ping");
                socket.send(WebSocketIoMessage::Pong(data)).await?;
                self.handler.on_ping(self.context.clone()).await
            }
            WebSocketIoMessage::Pong(_) => {
                debug!("Received pong");
                self.handler.on_pong(self.context.clone()).await
            }
            WebSocketIoMessage::Close(reason) => {
                debug!("Received close frame: {:?}", reason);
                self.handler
                    .on_disconnect(self.context.clone(), reason.clone())
                    .await?;
                Err(ServerError::WebSocketError("Connection closed".to_string()))
            }
        }
    }

    /// Handle text messages (JSON-RPC or bidirectional messages)
    async fn handle_text_message<S: WebSocketIo + ?Sized>(
        &mut self,
        text: String,
        socket: &mut S,
    ) -> ServerResult<()> {
        // Try to parse as BidirectionalMessage first
        if let Ok(msg) = serde_json::from_str::<BidirectionalMessage>(&text) {
            return self.handle_bidirectional_message(msg, socket).await;
        }

        // Try to parse as JSON-RPC request
        if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&text) {
            return self.handle_jsonrpc_request(request, socket).await;
        }

        // If neither worked, return error
        Err(ServerError::InvalidRequest(
            "Could not parse message as JSON-RPC or bidirectional message".to_string(),
        ))
    }

    /// Handle bidirectional messages
    async fn handle_bidirectional_message<S: WebSocketIo + ?Sized>(
        &mut self,
        msg: BidirectionalMessage,
        _socket: &mut S,
    ) -> ServerResult<()> {
        match msg {
            BidirectionalMessage::Request(request) => {
                // Handle as JSON-RPC request
                self.handle_jsonrpc_request(request, _socket).await
            }
            BidirectionalMessage::Subscribe { topics } => {
                self.handler
                    .handle_subscribe(topics, self.context.clone())
                    .await
            }
            BidirectionalMessage::Unsubscribe { topics } => {
                self.handler
                    .handle_unsubscribe(topics, self.context.clone())
                    .await
            }
            BidirectionalMessage::Ping => self.handler.on_ping(self.context.clone()).await,
            BidirectionalMessage::Pong => self.handler.on_pong(self.context.clone()).await,
            // Other message types are typically server-to-client
            _ => {
                warn!("Received unexpected bidirectional message type from client");
                Ok(())
            }
        }
    }

    /// Handle JSON-RPC requests
    async fn handle_jsonrpc_request<S: WebSocketIo + ?Sized>(
        &mut self,
        request: JsonRpcRequest,
        socket: &mut S,
    ) -> ServerResult<()> {
        debug!("Handling JSON-RPC request: {}", request.method);
        let request_id = request.id.clone();

        match self
            .handler
            .handle_request(request, self.context.clone())
            .await
        {
            Ok(Some(response)) => {
                // Send response back to client
                let response_msg = BidirectionalMessage::Response(response);
                self.send_message(socket, response_msg).await
            }
            Ok(None) => {
                // No response needed (notification)
                Ok(())
            }
            Err(e) => {
                error!("Error handling request: {}", e);
                let response =
                    JsonRpcResponse::error(jsonrpc_error_from_server_error(&e), request_id);
                self.send_message(socket, BidirectionalMessage::Response(response))
                    .await
            }
        }
    }

    /// Send a message to the WebSocket client
    async fn send_message<S: WebSocketIo + ?Sized>(
        &self,
        socket: &mut S,
        msg: BidirectionalMessage,
    ) -> ServerResult<()> {
        let json = serde_json::to_string(&msg)?;
        socket.send(WebSocketIoMessage::Text(json)).await
    }
}

fn jsonrpc_error_from_server_error(error: &ServerError) -> JsonRpcError {
    let code = match error {
        ServerError::AuthenticationFailed(_) => error_codes::AUTHENTICATION_REQUIRED,
        ServerError::PermissionDenied(_) => error_codes::INSUFFICIENT_PERMISSIONS,
        ServerError::InvalidRequest(_) => error_codes::INVALID_REQUEST,
        ServerError::HandlerNotFound(_) => error_codes::METHOD_NOT_FOUND,
        ServerError::SerializationError(_) => error_codes::INVALID_PARAMS,
        ServerError::UpgradeFailed(_)
        | ServerError::ConnectionNotFound(_)
        | ServerError::RoutingFailed(_)
        | ServerError::WebSocketError(_)
        | ServerError::ConnectionError(_)
        | ServerError::Internal(_) => error_codes::INTERNAL_ERROR,
    };

    JsonRpcError::new(code, error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ChannelMessageSender;
    use ras_jsonrpc_bidirectional_types::ConnectionId;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A minimal MessageHandler that only implements the required method —
    /// every other method falls through to the default impl, which is what
    /// these tests are verifying.
    struct PassThrough;

    #[async_trait]
    impl MessageHandler for PassThrough {
        async fn handle_request(
            &self,
            _request: JsonRpcRequest,
            _context: Arc<ConnectionContext>,
        ) -> ServerResult<Option<JsonRpcResponse>> {
            Ok(None)
        }
    }

    struct RespondingHandler;

    #[async_trait]
    impl MessageHandler for RespondingHandler {
        async fn handle_request(
            &self,
            request: JsonRpcRequest,
            _context: Arc<ConnectionContext>,
        ) -> ServerResult<Option<JsonRpcResponse>> {
            Ok(Some(JsonRpcResponse::success(
                serde_json::json!({
                    "method": request.method,
                    "params": request.params,
                }),
                request.id,
            )))
        }
    }

    struct RecoveringHandler;

    #[async_trait]
    impl MessageHandler for RecoveringHandler {
        async fn handle_request(
            &self,
            request: JsonRpcRequest,
            _context: Arc<ConnectionContext>,
        ) -> ServerResult<Option<JsonRpcResponse>> {
            if request.method == "fail" {
                return Err(ServerError::InvalidRequest("bad request".into()));
            }

            Ok(Some(JsonRpcResponse::success(
                serde_json::json!({
                    "method": request.method,
                }),
                request.id,
            )))
        }
    }

    struct RecordingLifecycle {
        disconnect_reasons: Mutex<Vec<Option<String>>>,
    }

    impl RecordingLifecycle {
        fn new() -> Self {
            Self {
                disconnect_reasons: Mutex::new(Vec::new()),
            }
        }

        fn disconnect_reasons(&self) -> Vec<Option<String>> {
            self.disconnect_reasons
                .lock()
                .expect("disconnect reasons lock")
                .clone()
        }
    }

    #[async_trait]
    impl MessageHandler for RecordingLifecycle {
        async fn handle_request(
            &self,
            _request: JsonRpcRequest,
            _context: Arc<ConnectionContext>,
        ) -> ServerResult<Option<JsonRpcResponse>> {
            Ok(None)
        }

        async fn on_disconnect(
            &self,
            _context: Arc<ConnectionContext>,
            reason: Option<String>,
        ) -> ServerResult<()> {
            self.disconnect_reasons
                .lock()
                .expect("disconnect reasons lock")
                .push(reason);
            Ok(())
        }
    }

    struct InMemorySocket {
        incoming: VecDeque<WebSocketIoMessage>,
        outgoing: Vec<WebSocketIoMessage>,
        close_when_empty: bool,
    }

    impl InMemorySocket {
        fn closing(incoming: impl IntoIterator<Item = WebSocketIoMessage>) -> Self {
            Self {
                incoming: incoming.into_iter().collect(),
                outgoing: Vec::new(),
                close_when_empty: true,
            }
        }

        fn pending() -> Self {
            Self {
                incoming: VecDeque::new(),
                outgoing: Vec::new(),
                close_when_empty: false,
            }
        }
    }

    #[async_trait]
    impl WebSocketIo for InMemorySocket {
        async fn send(&mut self, message: WebSocketIoMessage) -> ServerResult<()> {
            self.outgoing.push(message);
            Ok(())
        }

        async fn recv(&mut self) -> Option<ServerResult<WebSocketIoMessage>> {
            if let Some(message) = self.incoming.pop_front() {
                return Some(Ok(message));
            }

            if self.close_when_empty {
                None
            } else {
                std::future::pending::<Option<ServerResult<WebSocketIoMessage>>>().await
            }
        }
    }

    fn ctx() -> Arc<ConnectionContext> {
        let id = ConnectionId::new();
        let (tx, _rx) = mpsc::channel(4);
        let sender = ChannelMessageSender::new(id, tx);
        Arc::new(ConnectionContext::new(id, sender))
    }

    #[tokio::test]
    async fn default_handle_subscribe_writes_to_context() {
        let h = PassThrough;
        let c = ctx();
        h.handle_subscribe(vec!["a".into(), "b".into()], c.clone())
            .await
            .unwrap();
        assert!(c.is_subscribed_to("a").await);
        assert!(c.is_subscribed_to("b").await);
    }

    #[tokio::test]
    async fn default_handle_unsubscribe_removes_from_context() {
        let h = PassThrough;
        let c = ctx();
        c.subscribe("a".into()).await;
        c.subscribe("b".into()).await;
        h.handle_unsubscribe(vec!["a".into()], c.clone())
            .await
            .unwrap();
        assert!(!c.is_subscribed_to("a").await);
        assert!(c.is_subscribed_to("b").await);
    }

    #[tokio::test]
    async fn default_lifecycle_methods_succeed() {
        let h = PassThrough;
        let c = ctx();
        h.on_connect(c.clone()).await.unwrap();
        h.on_ping(c.clone()).await.unwrap();
        h.on_pong(c.clone()).await.unwrap();
        h.on_disconnect(c.clone(), Some("bye".into()))
            .await
            .unwrap();
        // None reason path too.
        h.on_disconnect(c, None).await.unwrap();
    }

    #[tokio::test]
    async fn handler_loop_processes_jsonrpc_request_without_socket() {
        let request = JsonRpcRequest::new(
            "echo".into(),
            Some(serde_json::json!({"value": 42})),
            Some(serde_json::json!(7)),
        );
        let incoming = serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap();
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text(incoming)]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(RespondingHandler), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        let response = match &messages[1] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected response, got {other:?}"),
        };
        assert_eq!(response.id, Some(serde_json::json!(7)));
        assert_eq!(response.result.as_ref().unwrap()["method"], "echo");
        assert_eq!(response.result.as_ref().unwrap()["params"]["value"], 42);

        assert!(matches!(
            messages[2],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_sends_jsonrpc_error_and_continues_without_socket() {
        let fail = JsonRpcRequest::new(
            "fail".into(),
            Some(serde_json::json!({})),
            Some(serde_json::json!(1)),
        );
        let ok = JsonRpcRequest::new(
            "ok".into(),
            Some(serde_json::json!({})),
            Some(serde_json::json!(2)),
        );
        let mut socket = InMemorySocket::closing([
            WebSocketIoMessage::Text(
                serde_json::to_string(&BidirectionalMessage::Request(fail)).unwrap(),
            ),
            WebSocketIoMessage::Text(
                serde_json::to_string(&BidirectionalMessage::Request(ok)).unwrap(),
            ),
        ]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(RecoveringHandler), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        let error_response = match &messages[1] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected error response, got {other:?}"),
        };
        assert_eq!(error_response.id, Some(serde_json::json!(1)));
        let error = error_response.error.as_ref().expect("JSON-RPC error");
        assert_eq!(error.code, ras_jsonrpc_types::error_codes::INVALID_REQUEST);
        assert_eq!(error.message, "Invalid request: bad request");

        let success_response = match &messages[2] {
            BidirectionalMessage::Response(response) => response,
            other => panic!("expected success response, got {other:?}"),
        };
        assert_eq!(success_response.id, Some(serde_json::json!(2)));
        assert_eq!(success_response.result.as_ref().unwrap()["method"], "ok");

        assert!(matches!(
            messages[3],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_processes_control_messages_without_socket() {
        let context = ctx();
        let subscribe = serde_json::to_string(&BidirectionalMessage::Subscribe {
            topics: vec!["room:1".into()],
        })
        .unwrap();
        let unsubscribe = serde_json::to_string(&BidirectionalMessage::Unsubscribe {
            topics: vec!["room:1".into()],
        })
        .unwrap();
        let mut socket = InMemorySocket::closing([
            WebSocketIoMessage::Text(subscribe),
            WebSocketIoMessage::Text(unsubscribe),
            WebSocketIoMessage::Ping(vec![1, 2, 3]),
        ]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), context.clone(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(!context.is_subscribed_to("room:1").await);
        assert!(
            socket
                .outgoing
                .contains(&WebSocketIoMessage::Pong(vec![1, 2, 3]))
        );
    }

    #[tokio::test]
    async fn handler_loop_sends_manager_messages_without_socket() {
        let notification = BidirectionalMessage::ServerNotification(
            ras_jsonrpc_bidirectional_types::ServerNotification {
                method: "server.note".into(),
                params: serde_json::json!({"ok": true}),
                metadata: None,
            },
        );
        let (tx, rx) = mpsc::channel(4);
        tx.send(notification).await.unwrap();
        drop(tx);

        let mut socket = InMemorySocket::pending();
        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));

        match &messages[1] {
            BidirectionalMessage::ServerNotification(notification) => {
                assert_eq!(notification.method, "server.note");
                assert_eq!(notification.params["ok"], true);
            }
            other => panic!("expected server notification, got {other:?}"),
        }

        assert!(matches!(
            messages[2],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_closes_malformed_text_without_response() {
        let mut socket =
            InMemorySocket::closing([WebSocketIoMessage::Text("not json-rpc".to_string())]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));
        assert!(matches!(
            messages[1],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_closes_oversized_text_without_response() {
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text("too large".into())]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 4)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));
        assert!(matches!(
            messages[1],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_ignores_non_utf8_binary_without_response() {
        let mut socket = InMemorySocket::closing([WebSocketIoMessage::Binary(vec![0xff, 0xfe])]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        let messages = bidirectional_outgoing(&socket);
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            messages[0],
            BidirectionalMessage::ConnectionEstablished { .. }
        ));
        assert!(matches!(
            messages[1],
            BidirectionalMessage::ConnectionClosed { .. }
        ));
    }

    #[tokio::test]
    async fn handler_loop_records_close_reason_without_socket() {
        let handler = Arc::new(RecordingLifecycle::new());
        let mut socket =
            InMemorySocket::closing([WebSocketIoMessage::Close(Some("client bye".to_string()))]);
        let (_tx, rx) = mpsc::channel(4);

        WebSocketHandler::new(handler.clone(), ctx(), rx, 1024)
            .run_with_io(&mut socket)
            .await
            .unwrap();

        assert!(
            handler
                .disconnect_reasons()
                .contains(&Some("client bye".to_string()))
        );
    }

    fn bidirectional_outgoing(socket: &InMemorySocket) -> Vec<BidirectionalMessage> {
        socket
            .outgoing
            .iter()
            .filter_map(|message| match message {
                WebSocketIoMessage::Text(text) => serde_json::from_str(text).ok(),
                _ => None,
            })
            .collect()
    }
}
