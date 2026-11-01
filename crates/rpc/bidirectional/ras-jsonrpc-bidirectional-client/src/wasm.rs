//! WASM WebSocket transport implementation using web-sys

use crate::inbound::{InboundOverflow, InboundQueue};
use crate::{
    WebSocketTransport,
    config::ClientConfig,
    error::{ClientError, ClientResult},
};
use async_trait::async_trait;
use futures::channel::oneshot;
use js_sys::Uint8Array;
use ras_jsonrpc_bidirectional_types::BidirectionalMessage;
use std::sync::{Arc, Mutex};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// WASM WebSocket transport using web-sys.
///
/// Received messages wait in a bounded queue until the client handles them:
/// at most `ClientConfig::message_buffer_size` messages and 64 MiB, and no
/// single message over 16 MiB, the native transport's limit. A server that
/// goes past that is disconnected rather than allowed to fill the page's
/// memory.
pub struct WasmWebSocketTransport {
    websocket: Arc<Mutex<Option<WebSocket>>>,
    message_queue: Arc<Mutex<InboundQueue>>,
    connection_state: Arc<Mutex<WasmConnectionState>>,
    /// Event callbacks of the current socket, owned so they are released on
    /// disconnect instead of leaking one set per connection.
    event_handlers: Option<EventHandlers>,
    url: String,
    /// Subprotocols offered on upgrade; carries the bearer token in browsers
    protocols: Vec<String>,
}

struct EventHandlers {
    _onopen: Closure<dyn FnMut(JsValue)>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onerror: Closure<dyn FnMut(ErrorEvent)>,
    _onclose: Closure<dyn FnMut(CloseEvent)>,
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
        let protocols = config.get_subprotocols();

        Self {
            websocket: Arc::new(Mutex::new(None)),
            message_queue: Arc::new(Mutex::new(InboundQueue::new(config.message_buffer_size))),
            connection_state: Arc::new(Mutex::new(WasmConnectionState::Disconnected)),
            event_handlers: None,
            url,
            protocols,
        }
    }

    /// Set up WebSocket event handlers
    fn setup_event_handlers(
        &self,
        websocket: &WebSocket,
        connect_tx: oneshot::Sender<ClientResult<()>>,
    ) -> EventHandlers {
        let message_queue = Arc::clone(&self.message_queue);
        let connection_state = Arc::clone(&self.connection_state);
        let connect_tx = Arc::new(Mutex::new(Some(connect_tx)));

        let onopen = {
            let connection_state = Arc::clone(&connection_state);
            let connect_tx = Arc::clone(&connect_tx);
            Closure::wrap(Box::new(move |_event: JsValue| {
                *connection_state.lock().unwrap() = WasmConnectionState::Connected;
                if let Some(tx) = connect_tx.lock().unwrap().take() {
                    let _ = tx.send(Ok(()));
                }
            }) as Box<dyn FnMut(JsValue)>)
        };
        websocket.set_onopen(Some(onopen.as_ref().unchecked_ref()));

        let onmessage = {
            let message_queue = Arc::clone(&message_queue);
            let connection_state = Arc::clone(&connection_state);
            let websocket = Arc::clone(&self.websocket);
            Closure::wrap(Box::new(move |event: MessageEvent| {
                let queued = match Self::parse_message_event(&event) {
                    Ok(Some((message, size))) => message_queue.lock().unwrap().push(message, size),
                    Ok(None) => Ok(()),
                    Err(overflow) => Err(overflow),
                };
                if let Err(overflow) = queued {
                    // Disconnect rather than buffer without bound.
                    message_queue.lock().unwrap().clear();
                    *connection_state.lock().unwrap() =
                        WasmConnectionState::Error(overflow.to_string());
                    if let Some(websocket) = websocket.lock().unwrap().as_ref() {
                        let _ = websocket.close();
                    }
                }
            }) as Box<dyn FnMut(MessageEvent)>)
        };
        websocket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let onerror = {
            let connection_state = Arc::clone(&connection_state);
            let connect_tx = Arc::clone(&connect_tx);
            Closure::wrap(Box::new(move |event: ErrorEvent| {
                let error_msg = format!("WebSocket error: {}", event.message());
                *connection_state.lock().unwrap() = WasmConnectionState::Error(error_msg.clone());
                if let Some(tx) = connect_tx.lock().unwrap().take() {
                    let _ = tx.send(Err(ClientError::javascript(error_msg)));
                }
            }) as Box<dyn FnMut(ErrorEvent)>)
        };
        websocket.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        let onclose = {
            let connection_state = Arc::clone(&connection_state);
            let connect_tx = Arc::clone(&connect_tx);
            Closure::wrap(Box::new(move |event: CloseEvent| {
                let mut state = connection_state.lock().unwrap();
                // Keep the reason if the transport closed the socket itself.
                if !matches!(*state, WasmConnectionState::Error(_)) {
                    *state = WasmConnectionState::Closed;
                }
                if let Some(tx) = connect_tx.lock().unwrap().take() {
                    let error_msg =
                        format!("Connection closed: {} - {}", event.code(), event.reason());
                    let _ = tx.send(Err(ClientError::connection(error_msg)));
                }
            }) as Box<dyn FnMut(CloseEvent)>)
        };
        websocket.set_onclose(Some(onclose.as_ref().unchecked_ref()));

        EventHandlers {
            _onopen: onopen,
            _onmessage: onmessage,
            _onerror: onerror,
            _onclose: onclose,
        }
    }

    /// Detach the event callbacks from `websocket` so they can be dropped.
    fn detach_event_handlers(websocket: &WebSocket) {
        websocket.set_onopen(None);
        websocket.set_onmessage(None);
        websocket.set_onerror(None);
        websocket.set_onclose(None);
    }

    /// Parse a WebSocket message event into a message and its size in bytes.
    ///
    /// `Ok(None)` is a message that is ignored: not JSON the client knows,
    /// or of an unsupported type. An oversized message is an overflow.
    fn parse_message_event(
        event: &MessageEvent,
    ) -> Result<Option<(BidirectionalMessage, usize)>, InboundOverflow> {
        let data = event.data();

        if let Some(text) = data.as_string() {
            InboundQueue::check_size(text.len())?;
            return Ok(serde_json::from_str(&text)
                .ok()
                .map(|message| (message, text.len())));
        }

        if let Ok(array_buffer) = data.dyn_into::<js_sys::ArrayBuffer>() {
            InboundQueue::check_size(array_buffer.byte_length() as usize)?;
            let bytes = Uint8Array::new(&array_buffer).to_vec();
            return Ok(serde_json::from_slice(&bytes)
                .ok()
                .map(|message| (message, bytes.len())));
        }

        Ok(None)
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

        // Create WebSocket, offering the subprotocol list (browsers cannot set
        // headers, so `token.<jwt>` rides here; the server selects `ras-jsonrpc`)
        let protocol_list = js_sys::Array::new();
        for protocol in &self.protocols {
            protocol_list.push(&JsValue::from_str(protocol));
        }
        let websocket = WebSocket::new_with_str_sequence(&self.url, &protocol_list)
            .map_err(|e| ClientError::javascript(format!("Failed to create WebSocket: {:?}", e)))?;

        // Set binary type to arraybuffer for better binary message handling
        websocket.set_binary_type(BinaryType::Arraybuffer);

        let (connect_tx, connect_rx) = oneshot::channel();

        // A socket the server closed is still attached to the old callbacks.
        if let Some(old) = self.websocket.lock().unwrap().take() {
            Self::detach_event_handlers(&old);
            let _ = old.close();
        }
        self.message_queue.lock().unwrap().clear();
        self.event_handlers = Some(self.setup_event_handlers(&websocket, connect_tx));

        *self.websocket.lock().unwrap() = Some(websocket);

        connect_rx
            .await
            .map_err(|_| ClientError::internal("Connection channel closed"))?
    }

    async fn disconnect(&mut self) -> ClientResult<()> {
        let websocket = self.websocket.lock().unwrap().take();

        if let Some(websocket) = websocket {
            *self.connection_state.lock().unwrap() = WasmConnectionState::Closing;

            Self::detach_event_handlers(&websocket);
            websocket.close().map_err(|e| {
                ClientError::javascript(format!("Failed to close WebSocket: {:?}", e))
            })?;
        }
        // Detached above, so the browser can no longer call them.
        self.event_handlers = None;

        *self.connection_state.lock().unwrap() = WasmConnectionState::Disconnected;
        self.message_queue.lock().unwrap().clear();

        Ok(())
    }

    async fn send(&mut self, message: &BidirectionalMessage) -> ClientResult<()> {
        let json = serde_json::to_string(message).map_err(ClientError::Json)?;

        self.send_data(json.as_bytes())
    }

    async fn receive(&mut self) -> ClientResult<Option<BidirectionalMessage>> {
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

        let message = self.message_queue.lock().unwrap().pop();
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

impl Drop for WasmWebSocketTransport {
    fn drop(&mut self) {
        // The callbacks are dropped with the transport; the browser must not
        // call them afterwards.
        if let Some(websocket) = self.websocket.lock().unwrap().take() {
            Self::detach_event_handlers(&websocket);
            let _ = websocket.close();
        }
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
