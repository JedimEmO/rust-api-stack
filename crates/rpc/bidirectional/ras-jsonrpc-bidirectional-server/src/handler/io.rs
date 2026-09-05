//! Socket IO boundary and Axum adapter.

use crate::{ServerError, ServerResult};
use async_trait::async_trait;
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::stream::StreamExt;

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
