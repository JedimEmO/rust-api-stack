//! WASM WebSocket transport implementation using web-sys

use crate::{
    WebSocketTransport,
    config::ClientConfig,
    error::{ClientError, ClientResult},
};
use async_trait::async_trait;
use futures::channel::oneshot;
use js_sys::Uint8Array;
use ras_jsonrpc_bidirectional_types::BidirectionalMessage;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// WASM WebSocket transport using web-sys
pub struct WasmWebSocketTransport {
    websocket: Arc<Mutex<Option<WebSocket>>>,
    message_queue: Arc<Mutex<VecDeque<BidirectionalMessage>>>,
    connection_state: Arc<Mutex<WasmConnectionState>>,
    url: String,
}

#[derive(Debug, Clone, PartialEq)]
enum WasmConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Closing,
    Closed,
    Error(String),
}

impl WasmWebSocketTransport {
    /// Create a new WASM WebSocket transport
    pub fn new(config: ClientConfig) -> Self {
        let url = config.get_connection_url();

        Self {
            websocket: Arc::new(Mutex::new(None)),
            message_queue: Arc::new(Mutex::new(VecDeque::new())),
            connection_state: Arc::new(Mutex::new(WasmConnectionState::Disconnected)),
            url,
        }
    }

    /// Set up WebSocket event handlers
    fn setup_event_handlers(
        &self,
        websocket: &WebSocket,
        connect_tx: oneshot::Sender<ClientResult<()>>,
    ) -> ClientResult<()> {
        let message_queue = Arc::clone(&self.message_queue);
        let connection_state = Arc::clone(&self.connection_state);
        let connect_tx = Arc::new(Mutex::new(Some(connect_tx)));

        // Handle connection open
        {
            let connection_state = Arc::clone(&connection_state);
            let connect_tx = Arc::clone(&connect_tx);
            let onopen_callback = Closure::wrap(Box::new(move |_event: JsValue| {
                *connection_state.lock().unwrap() = WasmConnectionState::Connected;
                if let Some(tx) = connect_tx.lock().unwrap().take() {
                    let _ = tx.send(Ok(()));
                }
            }) as Box<dyn FnMut(JsValue)>);
            websocket.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
            onopen_callback.forget();
        }

        // Handle messages
        {
            let message_queue = Arc::clone(&message_queue);
            let onmessage_callback = Closure::wrap(Box::new(move |event: MessageEvent| {
                if let Ok(message) = Self::parse_message_event(&event) {
                    message_queue.lock().unwrap().push_back(message);
                }
            }) as Box<dyn FnMut(MessageEvent)>);
            websocket.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
            onmessage_callback.forget();
        }

        // Handle errors
        {
            let connection_state = Arc::clone(&connection_state);
            let connect_tx = Arc::clone(&connect_tx);
            let onerror_callback = Closure::wrap(Box::new(move |event: ErrorEvent| {
                let error_msg = format!("WebSocket error: {}", event.message());
                *connection_state.lock().unwrap() = WasmConnectionState::Error(error_msg.clone());
                if let Some(tx) = connect_tx.lock().unwrap().take() {
                    let _ = tx.send(Err(ClientError::javascript(error_msg)));
                }
            }) as Box<dyn FnMut(ErrorEvent)>);
            websocket.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
            onerror_callback.forget();
        }

        // Handle connection close
        {
            let connection_state = Arc::clone(&connection_state);
            let connect_tx = Arc::clone(&connect_tx);
            let onclose_callback = Closure::wrap(Box::new(move |event: CloseEvent| {
                *connection_state.lock().unwrap() = WasmConnectionState::Closed;
                if let Some(tx) = connect_tx.lock().unwrap().take() {
                    let error_msg =
                        format!("Connection closed: {} - {}", event.code(), event.reason());
                    let _ = tx.send(Err(ClientError::connection(error_msg)));
                }
            }) as Box<dyn FnMut(CloseEvent)>);
            websocket.set_onclose(Some(onclose_callback.as_ref().unchecked_ref()));
            onclose_callback.forget();
        }

        Ok(())
    }

    /// Parse a WebSocket message event into a BidirectionalMessage
    fn parse_message_event(event: &MessageEvent) -> ClientResult<BidirectionalMessage> {
        let data = event.data();

        // Handle text messages
        if let Some(text) = data.as_string() {
            let message: BidirectionalMessage =
                serde_json::from_str(&text).map_err(ClientError::Json)?;
            return Ok(message);
        }

        // Handle binary messages (ArrayBuffer or Blob)
        if let Ok(array_buffer) = data.dyn_into::<js_sys::ArrayBuffer>() {
            let uint8_array = Uint8Array::new(&array_buffer);
            let bytes = uint8_array.to_vec();
            let message: BidirectionalMessage =
                serde_json::from_slice(&bytes).map_err(ClientError::Json)?;
            return Ok(message);
        }

        Err(ClientError::javascript("Unsupported message data type"))
    }

    /// Send data to the WebSocket
    fn send_data(&self, data: &[u8]) -> ClientResult<()> {
        let websocket_guard = self.websocket.lock().unwrap();
        if let Some(ref websocket) = *websocket_guard {
            // Send as text (JSON-RPC is typically text-based)
            let text = String::from_utf8(data.to_vec())
                .map_err(|_| ClientError::javascript("Invalid UTF-8 data"))?;

            websocket
                .send_with_str(&text)
                .map_err(|e| ClientError::javascript(format!("Failed to send message: {:?}", e)))?;

            Ok(())
        } else {
            Err(ClientError::NotConnected)
        }
    }
}

#[async_trait(?Send)]
impl WebSocketTransport for WasmWebSocketTransport {
    async fn connect(&mut self) -> ClientResult<()> {
        // Check if already connected
        {
            let state = self.connection_state.lock().unwrap();
            if matches!(
                *state,
                WasmConnectionState::Connected | WasmConnectionState::Connecting
            ) {
                return Err(ClientError::AlreadyConnected);
            }
        }

        *self.connection_state.lock().unwrap() = WasmConnectionState::Connecting;

        // Create WebSocket
        let websocket = WebSocket::new(&self.url)
            .map_err(|e| ClientError::javascript(format!("Failed to create WebSocket: {:?}", e)))?;

        // Set binary type to arraybuffer for better binary message handling
        websocket.set_binary_type(BinaryType::Arraybuffer);

        // Set up connection completion channel
        let (connect_tx, connect_rx) = oneshot::channel();

        // Set up event handlers
        self.setup_event_handlers(&websocket, connect_tx)?;

        // Store the WebSocket
        *self.websocket.lock().unwrap() = Some(websocket);

        // Wait for connection to complete or fail
        connect_rx
            .await
            .map_err(|_| ClientError::internal("Connection channel closed"))?
    }

    async fn disconnect(&mut self) -> ClientResult<()> {
        let websocket = self.websocket.lock().unwrap().take();

        if let Some(websocket) = websocket {
            *self.connection_state.lock().unwrap() = WasmConnectionState::Closing;

            // Close the WebSocket connection
            websocket.close().map_err(|e| {
                ClientError::javascript(format!("Failed to close WebSocket: {:?}", e))
            })?;
        }

        *self.connection_state.lock().unwrap() = WasmConnectionState::Disconnected;
        self.message_queue.lock().unwrap().clear();

        Ok(())
    }

    async fn send(&mut self, message: &BidirectionalMessage) -> ClientResult<()> {
        let json = serde_json::to_string(message).map_err(ClientError::Json)?;

        self.send_data(json.as_bytes())
    }

    async fn receive(&mut self) -> ClientResult<Option<BidirectionalMessage>> {
        // Check connection state
        {
            let state = self.connection_state.lock().unwrap();
            match *state {
                WasmConnectionState::Connected => {}
                WasmConnectionState::Error(ref error) => {
                    return Err(ClientError::connection(error.clone()));
                }
                WasmConnectionState::Closed => {
                    return Err(ClientError::connection("Connection closed"));
                }
                _ => {
                    return Err(ClientError::NotConnected);
                }
            }
        }

        // Try to get a message from the queue
        let message = self.message_queue.lock().unwrap().pop_front();
        Ok(message)
    }

    fn is_connected(&self) -> bool {
        let state = self.connection_state.lock().unwrap();
        matches!(*state, WasmConnectionState::Connected)
    }

    fn url(&self) -> &str {
        &self.url
    }
}

impl std::fmt::Debug for WasmWebSocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmWebSocketTransport")
            .field("url", &self.url)
            .field("is_connected", &self.is_connected())
            .field("state", &*self.connection_state.lock().unwrap())
            .finish()
    }
}

// Utility functions for WASM environment
pub mod utils {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: &str);
    }

    /// Log a message to the browser console
    pub fn console_log(message: &str) {
        log(message);
    }

    /// Get the current timestamp in milliseconds
    pub fn now() -> f64 {
        js_sys::Date::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ClientConfig;

    #[test]
    fn test_wasm_transport_creation() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let transport = WasmWebSocketTransport::new(config);

        assert_eq!(transport.url(), "ws://localhost:8080/ws");
        assert!(!transport.is_connected());
    }

    #[test]
    fn test_connection_state() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let transport = WasmWebSocketTransport::new(config);

        let state = transport.connection_state.lock().unwrap();
        assert_eq!(*state, WasmConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_disconnect_without_connection() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let mut transport = WasmWebSocketTransport::new(config);

        // Should not error when disconnecting without being connected
        let result = transport.disconnect().await;
        assert!(result.is_ok());
    }
}
