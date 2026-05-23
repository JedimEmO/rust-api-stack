//! Native WebSocket transport implementation using tokio-tungstenite

use crate::{
    WebSocketTransport,
    config::ClientConfig,
    error::{ClientError, ClientResult},
};
use async_trait::async_trait;
use futures::{FutureExt, SinkExt, StreamExt};
use ras_jsonrpc_bidirectional_types::BidirectionalMessage;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::Message,
    tungstenite::{handshake::client::generate_key, http::Request},
};
use tracing::{debug, info, warn};
use url::Url;

/// Native WebSocket transport using tokio-tungstenite
pub struct NativeWebSocketTransport {
    config: ClientConfig,
    connection: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    url: Url,
}

impl NativeWebSocketTransport {
    /// Create a new native WebSocket transport
    pub fn new(config: ClientConfig) -> Self {
        let url =
            Url::parse(&config.get_connection_url()).expect("URL should be validated in config");

        Self {
            config,
            connection: Arc::new(RwLock::new(None)),
            url,
        }
    }

    /// Build request headers for the WebSocket connection
    fn build_request_headers(&self) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();

        for (key, value) in self.config.get_connection_headers() {
            if let (Ok(header_name), Ok(header_value)) = (
                http::HeaderName::try_from(key),
                http::HeaderValue::try_from(value),
            ) {
                headers.insert(header_name, header_value);
            }
        }

        headers
    }

    fn host_header(&self) -> String {
        let host = self.url.host_str().unwrap_or("localhost");
        if let Some(port) = self.url.port() {
            format!("{}:{}", host, port)
        } else {
            host.to_string()
        }
    }

    fn build_connection_request(&self) -> Result<Request<()>, String> {
        let mut request = Request::builder()
            .method("GET")
            .uri(self.url.as_str())
            .header("Host", self.host_header())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key());

        let headers = self.build_request_headers();
        for (name, value) in headers.iter() {
            let header_name = name.as_str().to_lowercase();
            if !header_name.starts_with("sec-websocket")
                && header_name != "connection"
                && header_name != "upgrade"
                && header_name != "host"
            {
                request = request.header(name, value);
            }
        }

        request
            .body(())
            .map_err(|e| format!("Failed to build request: {}", e))
    }

    fn websocket_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
        let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
        config.max_message_size = Some(16 * 1024 * 1024); // 16MB
        config.max_frame_size = Some(16 * 1024 * 1024); // 16MB
        config.accept_unmasked_frames = false;
        config
    }
}

#[async_trait]
impl WebSocketTransport for NativeWebSocketTransport {
    async fn connect(&mut self) -> ClientResult<()> {
        info!("Connecting to WebSocket server: {}", self.url);

        let request = self
            .build_connection_request()
            .map_err(ClientError::connection)?;
        let config = Self::websocket_config();

        // Connect with timeout
        let connect_future = connect_async_with_config(request, Some(config), false);
        let (ws_stream, response) =
            tokio::time::timeout(self.config.connection_timeout, connect_future)
                .await
                .map_err(|_| ClientError::timeout(self.config.connection_timeout.as_secs()))?
                .map_err(|e| {
                    ClientError::connection(format!("WebSocket connection failed: {}", e))
                })?;

        debug!(
            "WebSocket connection established, status: {}",
            response.status()
        );

        // Store the connection
        *self.connection.write().await = Some(ws_stream);

        info!("Successfully connected to WebSocket server");
        Ok(())
    }

    async fn disconnect(&mut self) -> ClientResult<()> {
        info!("Disconnecting from WebSocket server");

        if let Some(mut ws) = self.connection.write().await.take() {
            // Send close frame
            if let Err(e) = ws.close(None).await {
                warn!("Error sending close frame: {}", e);
            }

            info!("WebSocket connection closed");
        }

        Ok(())
    }

    async fn send(&mut self, message: &BidirectionalMessage) -> ClientResult<()> {
        let json = serde_json::to_string(message).map_err(ClientError::Json)?;

        debug!("Sending message: {}", json);

        let ws_message = Message::Text(json.into());

        let mut connection_guard = self.connection.write().await;
        if let Some(ref mut ws) = *connection_guard {
            ws.send(ws_message)
                .await
                .map_err(|e| ClientError::send_failed(format!("Failed to send message: {}", e)))?;
            Ok(())
        } else {
            Err(ClientError::NotConnected)
        }
    }

    async fn receive(&mut self) -> ClientResult<Option<BidirectionalMessage>> {
        let mut connection_guard = self.connection.write().await;
        if let Some(ref mut ws) = *connection_guard {
            // Try to receive a message (non-blocking)
            match ws.next().now_or_never() {
                Some(Some(message)) => {
                    let message = message.map_err(|e| {
                        ClientError::receive_failed(format!("WebSocket error: {}", e))
                    })?;

                    match message {
                        Message::Text(text) => {
                            debug!("Received text message: {}", text);
                            let bidirectional_message: BidirectionalMessage =
                                serde_json::from_str(&text).map_err(ClientError::Json)?;
                            Ok(Some(bidirectional_message))
                        }
                        Message::Binary(data) => {
                            debug!("Received binary message ({} bytes)", data.len());
                            let bidirectional_message: BidirectionalMessage =
                                serde_json::from_slice(&data).map_err(ClientError::Json)?;
                            Ok(Some(bidirectional_message))
                        }
                        Message::Close(close_frame) => {
                            info!("Received close frame: {:?}", close_frame);
                            *self.connection.write().await = None;
                            Err(ClientError::connection("Connection closed by server"))
                        }
                        Message::Ping(data) => {
                            debug!("Received ping, sending pong");
                            if let Err(e) = ws.send(Message::Pong(data)).await {
                                warn!("Failed to send pong: {}", e);
                            }
                            Ok(None) // No message to return
                        }
                        Message::Pong(_) => {
                            debug!("Received pong");
                            Ok(None) // No message to return
                        }
                        Message::Frame(_) => {
                            // Raw frames are not expected in normal operation
                            warn!("Received unexpected raw frame");
                            Ok(None)
                        }
                    }
                }
                Some(None) => {
                    // Stream ended
                    info!("WebSocket stream ended");
                    *self.connection.write().await = None;
                    Err(ClientError::connection("WebSocket stream ended"))
                }
                None => {
                    // No message available right now
                    Ok(None)
                }
            }
        } else {
            Err(ClientError::NotConnected)
        }
    }

    fn is_connected(&self) -> bool {
        // The transport owns the stream while connected; absence means disconnect completed.
        futures::executor::block_on(async { self.connection.read().await.is_some() })
    }

    fn url(&self) -> &str {
        self.url.as_str()
    }
}

impl std::fmt::Debug for NativeWebSocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeWebSocketTransport")
            .field("url", &self.url.as_str())
            .field("is_connected", &self.is_connected())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, ClientConfig};

    #[test]
    fn test_native_transport_creation() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let transport = NativeWebSocketTransport::new(config);

        assert_eq!(transport.url(), "ws://localhost:8080/ws");
        assert!(!transport.is_connected());
    }

    #[test]
    fn test_build_request_headers() {
        let mut config = ClientConfig::new("ws://localhost:8080/ws");
        config
            .custom_headers
            .insert("X-Custom".to_string(), "value".to_string());

        let transport = NativeWebSocketTransport::new(config);
        let headers = transport.build_request_headers();

        assert!(headers.contains_key("X-Custom"));
    }

    #[test]
    fn build_request_headers_ignores_invalid_header_names_and_values() {
        let mut config = ClientConfig::new("ws://localhost:8080/ws");
        config
            .custom_headers
            .insert("bad header".to_string(), "ignored".to_string());
        config
            .custom_headers
            .insert("X-Bad-Value".to_string(), "line\r\nbreak".to_string());
        config
            .custom_headers
            .insert("X-Good".to_string(), "kept".to_string());

        let transport = NativeWebSocketTransport::new(config);
        let headers = transport.build_request_headers();

        assert!(!headers.contains_key("bad header"));
        assert!(!headers.contains_key("X-Bad-Value"));
        assert_eq!(headers.get("X-Good").unwrap(), "kept");
    }

    #[test]
    fn build_connection_request_sets_required_headers_and_preserves_auth_headers() {
        let mut config = ClientConfig::new("ws://example.test:9000/ws");
        config.auth = AuthConfig::JwtHeader {
            token: "secret".to_string(),
        };
        config
            .custom_headers
            .insert("X-Custom".to_string(), "value".to_string());
        config
            .custom_headers
            .insert("Host".to_string(), "malicious.example".to_string());
        config
            .custom_headers
            .insert("Connection".to_string(), "close".to_string());
        config.custom_headers.insert(
            "Sec-WebSocket-Key".to_string(),
            "not-the-generated-key".to_string(),
        );

        let transport = NativeWebSocketTransport::new(config);
        let request = transport.build_connection_request().unwrap();
        let headers = request.headers();

        assert_eq!(request.method(), "GET");
        assert_eq!(request.uri(), "ws://example.test:9000/ws");
        assert_eq!(headers.get("host").unwrap(), "example.test:9000");
        assert_eq!(headers.get("connection").unwrap(), "Upgrade");
        assert_eq!(headers.get("upgrade").unwrap(), "websocket");
        assert_eq!(headers.get("sec-websocket-version").unwrap(), "13");
        assert_ne!(
            headers.get("sec-websocket-key").unwrap(),
            "not-the-generated-key"
        );
        assert_eq!(headers.get("authorization").unwrap(), "Bearer secret");
        assert_eq!(headers.get("x-custom").unwrap(), "value");
    }

    #[test]
    fn build_connection_request_omits_port_from_host_header_when_url_has_no_port() {
        let config = ClientConfig::new("wss://example.test/ws");
        let transport = NativeWebSocketTransport::new(config);

        let request = transport.build_connection_request().unwrap();

        assert_eq!(request.headers().get("host").unwrap(), "example.test");
    }

    #[test]
    fn websocket_config_sets_expected_native_limits() {
        let config = NativeWebSocketTransport::websocket_config();

        assert_eq!(config.max_message_size, Some(16 * 1024 * 1024));
        assert_eq!(config.max_frame_size, Some(16 * 1024 * 1024));
        assert!(!config.accept_unmasked_frames);
    }

    #[tokio::test]
    async fn test_disconnect_without_connection() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let mut transport = NativeWebSocketTransport::new(config);

        // Should not error when disconnecting without being connected
        let result = transport.disconnect().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn send_without_connection_returns_not_connected() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let mut transport = NativeWebSocketTransport::new(config);

        let error = transport
            .send(&BidirectionalMessage::Ping)
            .await
            .expect_err("send should require a connection");

        assert!(matches!(error, ClientError::NotConnected));
    }

    #[tokio::test]
    async fn receive_without_connection_returns_not_connected() {
        let config = ClientConfig::new("ws://localhost:8080/ws");
        let mut transport = NativeWebSocketTransport::new(config);

        let error = transport
            .receive()
            .await
            .expect_err("receive should require a connection");

        assert!(matches!(error, ClientError::NotConnected));
    }
}
