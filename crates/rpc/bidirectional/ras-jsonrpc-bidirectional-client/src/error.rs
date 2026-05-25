//! Error types for the bidirectional JSON-RPC client

use ras_jsonrpc_bidirectional_types::BidirectionalError;
use thiserror::Error;

/// Errors that can occur in the bidirectional JSON-RPC client
#[derive(Error, Debug)]
pub enum ClientError {
    /// WebSocket connection error
    #[error("WebSocket connection error: {0}")]
    Connection(String),

    /// Authentication error
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Request timeout
    #[error("Request timeout after {timeout_seconds}s")]
    Timeout { timeout_seconds: u64 },

    /// Invalid request ID
    #[error("Invalid request ID: {0}")]
    InvalidRequestId(String),

    /// Client is not connected
    #[error("Client is not connected")]
    NotConnected,

    /// Client is already connected
    #[error("Client is already connected")]
    AlreadyConnected,

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Reconnection failed after maximum attempts
    #[error("Reconnection failed after {attempts} attempts")]
    ReconnectionFailed { attempts: u32 },

    /// Message sending failed
    #[error("Failed to send message: {0}")]
    SendFailed(String),

    /// Message receiving failed
    #[error("Failed to receive message: {0}")]
    ReceiveFailed(String),

    /// Subscription error
    #[error("Subscription error: {0}")]
    Subscription(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Bidirectional types error
    #[error("Bidirectional error: {0}")]
    Bidirectional(#[from] BidirectionalError),

    /// IO error (native only)
    #[cfg(not(target_arch = "wasm32"))]
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Tungstenite WebSocket error (native only)
    #[cfg(not(target_arch = "wasm32"))]
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    /// URL parsing error (native only)
    #[cfg(not(target_arch = "wasm32"))]
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// JavaScript error (WASM only)
    #[cfg(target_arch = "wasm32")]
    #[error("JavaScript error: {0}")]
    JavaScript(String),

    /// WASM binding error (WASM only)
    #[cfg(target_arch = "wasm32")]
    #[error("WASM binding error: {0}")]
    WasmBinding(String),
}

impl ClientError {
    /// Create a connection error
    pub fn connection<S: Into<String>>(msg: S) -> Self {
        Self::Connection(msg.into())
    }

    /// Create an authentication error
    pub fn authentication<S: Into<String>>(msg: S) -> Self {
        Self::Authentication(msg.into())
    }

    /// Create a timeout error
    pub fn timeout(timeout_seconds: u64) -> Self {
        Self::Timeout { timeout_seconds }
    }

    /// Create an invalid request ID error
    pub fn invalid_request_id<S: Into<String>>(id: S) -> Self {
        Self::InvalidRequestId(id.into())
    }

    /// Create an invalid URL error
    pub fn invalid_url<S: Into<String>>(url: S) -> Self {
        Self::InvalidUrl(url.into())
    }

    /// Create a reconnection failed error
    pub fn reconnection_failed(attempts: u32) -> Self {
        Self::ReconnectionFailed { attempts }
    }

    /// Create a send failed error
    pub fn send_failed<S: Into<String>>(msg: S) -> Self {
        Self::SendFailed(msg.into())
    }

    /// Create a receive failed error
    pub fn receive_failed<S: Into<String>>(msg: S) -> Self {
        Self::ReceiveFailed(msg.into())
    }

    /// Create a subscription error
    pub fn subscription<S: Into<String>>(msg: S) -> Self {
        Self::Subscription(msg.into())
    }

    /// Create a configuration error
    pub fn configuration<S: Into<String>>(msg: S) -> Self {
        Self::Configuration(msg.into())
    }

    /// Create an internal error
    pub fn internal<S: Into<String>>(msg: S) -> Self {
        Self::Internal(msg.into())
    }

    /// Check if this error is recoverable (connection can be retried)
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Connection(_)
                | Self::Timeout { .. }
                | Self::SendFailed(_)
                | Self::ReceiveFailed(_)
                | Self::NotConnected
        )
    }

    /// Check if this error should trigger a reconnection
    pub fn should_reconnect(&self) -> bool {
        matches!(
            self,
            Self::Connection(_) | Self::ReceiveFailed(_) | Self::NotConnected
        )
    }

    #[cfg(target_arch = "wasm32")]
    /// Create a JavaScript error (WASM only)
    pub fn javascript<S: Into<String>>(msg: S) -> Self {
        Self::JavaScript(msg.into())
    }

    #[cfg(target_arch = "wasm32")]
    /// Create a WASM binding error (WASM only)
    pub fn wasm_binding<S: Into<String>>(msg: S) -> Self {
        Self::WasmBinding(msg.into())
    }
}

/// Result type for client operations
pub type ClientResult<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = ClientError::connection("test connection error");
        assert!(matches!(err, ClientError::Connection(_)));
        assert_eq!(
            err.to_string(),
            "WebSocket connection error: test connection error"
        );
    }

    #[test]
    fn test_error_recovery() {
        let recoverable = ClientError::timeout(30);
        assert!(recoverable.is_recoverable());
        assert!(!recoverable.should_reconnect());

        let not_recoverable = ClientError::authentication("invalid token");
        assert!(!not_recoverable.is_recoverable());
        assert!(!not_recoverable.should_reconnect());

        let should_reconnect = ClientError::connection("lost connection");
        assert!(should_reconnect.is_recoverable());
        assert!(should_reconnect.should_reconnect());
    }

    #[test]
    fn test_timeout_error() {
        let err = ClientError::timeout(45);
        if let ClientError::Timeout { timeout_seconds } = err {
            assert_eq!(timeout_seconds, 45);
        } else {
            panic!("Expected timeout error");
        }
    }

    #[test]
    fn test_reconnection_failed() {
        let err = ClientError::reconnection_failed(3);
        if let ClientError::ReconnectionFailed { attempts } = err {
            assert_eq!(attempts, 3);
        } else {
            panic!("Expected reconnection failed error");
        }
    }

    #[test]
    fn covers_all_constructors_and_display() {
        // Stringy constructors → matching variants and messages.
        for (err, expected_prefix) in [
            (
                ClientError::invalid_request_id("rid"),
                "Invalid request ID:",
            ),
            (ClientError::invalid_url("not://valid"), "Invalid URL:"),
            (ClientError::send_failed("eof"), "Failed to send message:"),
            (
                ClientError::receive_failed("eof"),
                "Failed to receive message:",
            ),
            (ClientError::subscription("topic"), "Subscription error:"),
            (ClientError::configuration("bad"), "Configuration error:"),
            (ClientError::internal("oops"), "Internal error:"),
            (ClientError::authentication("nope"), "Authentication error:"),
        ] {
            let s = err.to_string();
            assert!(
                s.starts_with(expected_prefix),
                "expected prefix {expected_prefix:?} in {s:?}"
            );
        }

        // Bare variants.
        assert_eq!(
            ClientError::NotConnected.to_string(),
            "Client is not connected"
        );
        assert_eq!(
            ClientError::AlreadyConnected.to_string(),
            "Client is already connected"
        );
    }

    #[test]
    fn from_impls_route_to_correct_variants() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        assert!(matches!(ClientError::from(json_err), ClientError::Json(_)));

        let bidir_err = BidirectionalError::Timeout;
        assert!(matches!(
            ClientError::from(bidir_err),
            ClientError::Bidirectional(_)
        ));

        let io_err = std::io::Error::other("io");
        assert!(matches!(ClientError::from(io_err), ClientError::Io(_)));

        let url_err = url::Url::parse("not a url").unwrap_err();
        assert!(matches!(
            ClientError::from(url_err),
            ClientError::UrlParse(_)
        ));
    }

    #[test]
    fn recovery_classification_is_exhaustive_for_named_buckets() {
        // Should reconnect → also recoverable.
        for err in [
            ClientError::connection("x"),
            ClientError::receive_failed("x"),
            ClientError::NotConnected,
        ] {
            assert!(err.should_reconnect());
            assert!(err.is_recoverable());
        }

        // Recoverable but no reconnect.
        for err in [ClientError::timeout(1), ClientError::send_failed("x")] {
            assert!(err.is_recoverable());
            assert!(!err.should_reconnect());
        }

        // Neither.
        for err in [
            ClientError::authentication("x"),
            ClientError::AlreadyConnected,
            ClientError::invalid_url("x"),
            ClientError::configuration("x"),
            ClientError::internal("x"),
        ] {
            assert!(!err.is_recoverable());
            assert!(!err.should_reconnect());
        }
    }
}
