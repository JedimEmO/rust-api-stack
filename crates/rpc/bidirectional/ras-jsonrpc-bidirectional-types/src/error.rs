//! Error types for bidirectional JSON-RPC communication

use thiserror::Error;

/// Errors that can occur in bidirectional JSON-RPC communication
#[derive(Error, Debug)]
pub enum BidirectionalError {
    /// Connection not found
    #[error("Connection not found: {0}")]
    ConnectionNotFound(crate::ConnectionId),

    /// Connection already exists
    #[error("Connection already exists: {0}")]
    ConnectionAlreadyExists(crate::ConnectionId),

    /// Failed to send message
    #[error("Failed to send message: {0}")]
    SendError(String),

    /// Failed to broadcast message
    #[error("Failed to broadcast to topic '{topic}': {reason}")]
    BroadcastError { topic: String, reason: String },

    /// Authentication required
    #[error("Authentication required")]
    AuthenticationRequired,

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Invalid subscription topic
    #[error("Invalid subscription topic: {0}")]
    InvalidTopic(String),

    /// A subscription limit (per connection or global) would be exceeded
    #[error("Subscription limit reached: {0}")]
    SubscriptionLimitReached(String),

    /// Connection closed
    #[error("Connection closed")]
    ConnectionClosed,

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// WebSocket error
    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),

    /// Custom error
    #[error("{0}")]
    Custom(String),

    /// Connection error (for RPC calls)
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Request timeout
    #[error("Request timeout")]
    Timeout,

    /// RPC error response
    #[error("RPC error: {0}")]
    RpcError(String),

    /// Invalid response
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

impl BidirectionalError {
    /// Create a WebSocket error
    pub fn websocket<E: std::fmt::Display>(error: E) -> Self {
        Self::WebSocketError(error.to_string())
    }

    /// Create an internal error
    pub fn internal<E: std::fmt::Display>(error: E) -> Self {
        Self::InternalError(error.to_string())
    }

    /// Create a custom error
    pub fn custom<E: std::fmt::Display>(error: E) -> Self {
        Self::Custom(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConnectionId;

    #[test]
    fn helpers_wrap_display_into_correct_variant() {
        let id = ConnectionId::new();
        assert!(
            BidirectionalError::ConnectionNotFound(id)
                .to_string()
                .contains(&id.to_string())
        );
        assert_eq!(
            BidirectionalError::ConnectionAlreadyExists(id).to_string(),
            format!("Connection already exists: {id}")
        );
        assert_eq!(
            BidirectionalError::SendError("oops".into()).to_string(),
            "Failed to send message: oops"
        );
        assert_eq!(
            BidirectionalError::BroadcastError {
                topic: "t".into(),
                reason: "r".into(),
            }
            .to_string(),
            "Failed to broadcast to topic 't': r"
        );
        assert_eq!(
            BidirectionalError::AuthenticationRequired.to_string(),
            "Authentication required"
        );
        assert_eq!(
            BidirectionalError::PermissionDenied("admin".into()).to_string(),
            "Permission denied: admin"
        );
        assert_eq!(
            BidirectionalError::InvalidTopic("foo".into()).to_string(),
            "Invalid subscription topic: foo"
        );
        assert_eq!(
            BidirectionalError::ConnectionClosed.to_string(),
            "Connection closed"
        );
        assert_eq!(BidirectionalError::Timeout.to_string(), "Request timeout");
        assert_eq!(
            BidirectionalError::RpcError("nope".into()).to_string(),
            "RPC error: nope"
        );
        assert_eq!(
            BidirectionalError::InvalidResponse("garbage".into()).to_string(),
            "Invalid response: garbage"
        );
        assert_eq!(
            BidirectionalError::ConnectionError("eof".into()).to_string(),
            "Connection error: eof"
        );

        // Constructor helpers.
        let we = BidirectionalError::websocket("boom");
        assert!(matches!(we, BidirectionalError::WebSocketError(ref s) if s == "boom"));
        let ie = BidirectionalError::internal("ugh");
        assert!(matches!(ie, BidirectionalError::InternalError(ref s) if s == "ugh"));
        let ce = BidirectionalError::custom("plain");
        assert!(matches!(ce, BidirectionalError::Custom(ref s) if s == "plain"));
    }

    #[test]
    fn from_serde_json_error() {
        let parse_err = serde_json::from_str::<serde_json::Value>("{not json}").unwrap_err();
        let wrapped: BidirectionalError = parse_err.into();
        assert!(matches!(wrapped, BidirectionalError::SerializationError(_)));
        assert!(wrapped.to_string().starts_with("Serialization error:"));
    }
}
