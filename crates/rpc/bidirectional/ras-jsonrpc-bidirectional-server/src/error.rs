//! Error types for WebSocket server operations

use axum::http::StatusCode;
use ras_auth_core::AuthError;
use ras_jsonrpc_bidirectional_types::BidirectionalError;
use thiserror::Error;

/// Server-specific errors for WebSocket operations
#[derive(Debug, Error)]
pub enum ServerError {
    /// WebSocket upgrade failed
    #[error("WebSocket upgrade failed: {0}")]
    UpgradeFailed(String),

    /// Authentication failed during connection
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(#[from] AuthError),

    /// Connection not found
    #[error("Connection {0} not found")]
    ConnectionNotFound(String),

    /// Message routing failed
    #[error("Message routing failed: {0}")]
    RoutingFailed(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// WebSocket protocol error
    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    /// Connection management error
    #[error("Connection management error: {0}")]
    ConnectionError(#[from] BidirectionalError),

    /// Internal server error
    #[error("Internal server error: {0}")]
    Internal(String),

    /// Invalid request format
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Handler not found for method
    #[error("No handler found for method: {0}")]
    HandlerNotFound(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl ServerError {
    /// Generic, non-sensitive message safe to send to a client.
    ///
    /// The [`Display`](std::fmt::Display) impl keeps full detail for server logs;
    /// this is what goes on the wire (JSON-RPC error message / upgrade HTTP body)
    /// so handler internals, DSNs, auth specifics, or `AuthError` fields never
    /// leak to clients (H3). Mirrors `FileError::client_message`.
    pub fn client_message(&self) -> &'static str {
        match self {
            ServerError::AuthenticationFailed(_) => "Authentication failed",
            ServerError::PermissionDenied(_) => "Insufficient permissions",
            ServerError::ConnectionNotFound(_) => "Connection not found",
            ServerError::InvalidRequest(_) => "Invalid request",
            ServerError::HandlerNotFound(_) => "Method not found",
            ServerError::SerializationError(_) => "Invalid parameters",
            ServerError::UpgradeFailed(_)
            | ServerError::RoutingFailed(_)
            | ServerError::WebSocketError(_)
            | ServerError::ConnectionError(_)
            | ServerError::Internal(_) => "Internal error",
        }
    }

    /// Convert to HTTP status code for upgrade errors
    pub fn to_status_code(&self) -> StatusCode {
        match self {
            ServerError::AuthenticationFailed(_) => StatusCode::UNAUTHORIZED,
            ServerError::PermissionDenied(_) => StatusCode::FORBIDDEN,
            ServerError::ConnectionNotFound(_) => StatusCode::NOT_FOUND,
            ServerError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ServerError::HandlerNotFound(_) => StatusCode::NOT_IMPLEMENTED,
            ServerError::UpgradeFailed(_)
            | ServerError::RoutingFailed(_)
            | ServerError::SerializationError(_)
            | ServerError::WebSocketError(_)
            | ServerError::ConnectionError(_)
            | ServerError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Convenience type alias for server operation results
pub type ServerResult<T> = Result<T, ServerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_per_variant() {
        assert_eq!(
            ServerError::AuthenticationFailed(AuthError::InvalidToken).to_status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            ServerError::PermissionDenied("nope".into()).to_status_code(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            ServerError::ConnectionNotFound("abc".into()).to_status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ServerError::InvalidRequest("bad".into()).to_status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ServerError::HandlerNotFound("m".into()).to_status_code(),
            StatusCode::NOT_IMPLEMENTED
        );
        for variant in [
            ServerError::UpgradeFailed("x".into()),
            ServerError::RoutingFailed("x".into()),
            ServerError::WebSocketError("x".into()),
            ServerError::Internal("x".into()),
        ] {
            assert_eq!(variant.to_status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        // From impls
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let from_json: ServerError = json_err.into();
        assert_eq!(
            from_json.to_status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        let bidir_err: BidirectionalError = BidirectionalError::SendError("e".into());
        let from_bidir: ServerError = bidir_err.into();
        assert_eq!(
            from_bidir.to_status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );

        let auth_err: AuthError = AuthError::TokenExpired;
        let from_auth: ServerError = auth_err.into();
        assert_eq!(from_auth.to_status_code(), StatusCode::UNAUTHORIZED);

        // Display formatting: spot-check each kind once.
        assert!(
            ServerError::UpgradeFailed("x".into())
                .to_string()
                .starts_with("WebSocket upgrade failed:")
        );
    }

    #[test]
    fn client_message_never_leaks_internal_detail() {
        // Handler internals must not reach the client (H3).
        let internal = ServerError::Internal("database password is hunter2".into());
        assert_eq!(internal.client_message(), "Internal error");
        assert!(!internal.client_message().contains("hunter2"));

        // AuthError detail (DSNs, Internal(...) strings) must not reach the client.
        let auth = ServerError::AuthenticationFailed(AuthError::Internal(
            "dsn=postgres://user:pw@host/db".into(),
        ));
        assert_eq!(auth.client_message(), "Authentication failed");
        assert!(!auth.client_message().contains("dsn"));

        // But the full detail is still available in Display for server logs.
        assert!(internal.to_string().contains("hunter2"));
        assert!(auth.to_string().contains("dsn=postgres"));
    }
}
