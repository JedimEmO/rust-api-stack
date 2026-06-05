//! Typed transport error.
//!
//! Generated clients return `Result<T, TransportError>` instead of the old
//! `Box<dyn std::error::Error + Send + Sync>`, so callers can match on the
//! failure mode (connection vs. HTTP status vs. (de)serialization vs. a
//! JSON-RPC application error).

use thiserror::Error;

/// Errors produced by an [`crate::HttpTransport`] or by the helpers that build
/// requests / decode responses.
#[derive(Debug, Error)]
pub enum TransportError {
    /// The request never produced an HTTP response (DNS, TCP, TLS, connect, etc).
    #[error("connection error: {0}")]
    Connection(String),

    /// The request exceeded its configured timeout before a response arrived.
    #[error("timeout: {0}")]
    Timeout(String),

    /// The server returned a non-success HTTP status.
    ///
    /// Produced by [`crate::TransportResponse::error_for_status`]; transports
    /// themselves never inspect status (they are dumb pipes).
    #[error("http status {status}: {body}")]
    Status {
        /// The HTTP status code.
        status: http::StatusCode,
        /// The (best-effort, lossy-UTF8) response body captured for diagnostics.
        body: String,
    },

    /// Serializing a request body / query value failed.
    #[error("serialize error: {0}")]
    Serialize(#[source] serde_json::Error),

    /// Deserializing a response body failed.
    #[error("deserialize error: {0}")]
    Deserialize(#[source] serde_json::Error),

    /// Reading or writing a streaming body failed mid-flight.
    #[error("body error: {0}")]
    Body(String),

    /// A JSON-RPC 2.0 error object was returned in the response envelope.
    #[error("json-rpc error {code}: {message}")]
    JsonRpc {
        /// The JSON-RPC error code.
        code: i64,
        /// The JSON-RPC error message.
        message: String,
    },
}

impl TransportError {
    /// Construct a [`TransportError::Status`] from a status code and a body.
    pub fn http_status(status: http::StatusCode, body: impl Into<String>) -> Self {
        TransportError::Status {
            status,
            body: body.into(),
        }
    }
}

#[cfg(feature = "reqwest")]
impl From<reqwest::Error> for TransportError {
    fn from(err: reqwest::Error) -> Self {
        // Timeouts get their own variant (the per-request timeout this crate
        // plumbs through surfaces here); decode/body-read failures map to
        // `Body`; everything else is treated as a connection-level problem.
        if err.is_timeout() {
            TransportError::Timeout(err.to_string())
        } else if err.is_decode() {
            TransportError::Body(err.to_string())
        } else {
            TransportError::Connection(err.to_string())
        }
    }
}
