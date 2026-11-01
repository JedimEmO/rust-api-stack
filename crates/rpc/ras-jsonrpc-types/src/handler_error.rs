//! Errors a handler sends to the client on purpose.
//!
//! A handler returns `Box<dyn Error + Send + Sync>`, and by default every such
//! error reaches the client as `-32603 "Internal error"` with no detail. A
//! [`PublicRpcError`] returned directly is the one exception: its code, message
//! and data are sent as written.

use crate::{JsonRpcError, error_codes};
use std::fmt;

/// An error a handler deliberately sends to the client.
///
/// Return it directly, for instance with
/// `Err(PublicRpcError::new(1001, "Email already registered").into())`.
/// Only the error the handler returns is checked, not errors it wraps,
/// so an upstream error forwarded with `?` stays hidden even when it carries
/// JSON-RPC detail; forwarding one means converting it to a `PublicRpcError`.
///
/// The code must pass [`handler_code_allowed`]; any other code is logged and
/// replaced by `-32603`.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicRpcError {
    /// The JSON-RPC error code.
    pub code: i32,
    /// The message the client sees.
    pub message: String,
    /// Optional structured detail the client sees.
    pub data: Option<serde_json::Value>,
}

impl PublicRpcError {
    /// Creates an error with a code and message, and no data.
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Attaches structured detail.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

impl fmt::Display for PublicRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for PublicRpcError {}

/// Whether a handler may send `code`.
///
/// JSON-RPC reserves `-32768..=-32000` for the protocol and the server, and
/// this framework gives several of those codes meaning: the HTTP layer answers
/// `-32001` to `-32004` with 401 or 403. A handler may use any code outside
/// that range, and `-32602` (invalid params) for its own validation.
pub const fn handler_code_allowed(code: i32) -> bool {
    code == error_codes::INVALID_PARAMS || code < -32768 || code > -32000
}

/// What a handler's error becomes on the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum HandlerErrorOutcome {
    /// Sent as written.
    Public(JsonRpcError),
    /// A public error with a reserved code; sent as `-32603`, and worth logging.
    ReservedCode(i32),
    /// Any other error; sent as `-32603` with no detail.
    Hidden,
}

impl HandlerErrorOutcome {
    /// Classifies the error a handler returned.
    ///
    /// Only a [`PublicRpcError`] that *is* the returned error counts;
    /// its `source()` chain and any wrapper around it are not searched.
    pub fn from_handler_error(error: &(dyn std::error::Error + Send + Sync + 'static)) -> Self {
        match error.downcast_ref::<PublicRpcError>() {
            Some(public) => Self::from_wire(JsonRpcError::new(
                public.code,
                public.message.clone(),
                public.data.clone(),
            )),
            None => Self::Hidden,
        }
    }

    /// Checks an error that already has wire form, such as one a WebSocket
    /// handler path carries as a `JsonRpcError`.
    pub fn from_wire(error: JsonRpcError) -> Self {
        if handler_code_allowed(error.code) {
            Self::Public(error)
        } else {
            Self::ReservedCode(error.code)
        }
    }

    /// The error to send.
    pub fn into_error(self) -> JsonRpcError {
        match self {
            Self::Public(error) => error,
            Self::ReservedCode(_) | Self::Hidden => JsonRpcError::internal_error(String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outcome(error: Box<dyn std::error::Error + Send + Sync>) -> HandlerErrorOutcome {
        HandlerErrorOutcome::from_handler_error(error.as_ref())
    }

    #[test]
    fn code_boundaries() {
        assert!(!handler_code_allowed(-32768));
        assert!(!handler_code_allowed(-32000));
        assert!(!handler_code_allowed(error_codes::AUTHENTICATION_REQUIRED));
        assert!(!handler_code_allowed(error_codes::INTERNAL_ERROR));
        assert!(handler_code_allowed(-32769));
        assert!(handler_code_allowed(-31999));
        assert!(handler_code_allowed(error_codes::INVALID_PARAMS));
        assert!(handler_code_allowed(0));
        assert!(handler_code_allowed(1001));
    }

    #[test]
    fn a_returned_public_error_is_sent_as_written() {
        let error = PublicRpcError::new(1001, "Email already registered")
            .with_data(json!({ "field": "email" }));
        let sent = outcome(Box::new(error)).into_error();
        assert_eq!(sent.code, 1001);
        assert_eq!(sent.message, "Email already registered");
        assert_eq!(sent.data, Some(json!({ "field": "email" })));
    }

    #[test]
    fn a_reserved_code_is_reported_and_hidden() {
        let result = outcome(Box::new(PublicRpcError::new(-32001, "fake auth failure")));
        assert_eq!(result, HandlerErrorOutcome::ReservedCode(-32001));
        let sent = result.into_error();
        assert_eq!(sent.code, error_codes::INTERNAL_ERROR);
        assert_eq!(sent.message, "Internal error");
        assert_eq!(sent.data, None);
    }

    #[derive(Debug)]
    struct Upstream(PublicRpcError);

    impl fmt::Display for Upstream {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "upstream failed")
        }
    }

    impl std::error::Error for Upstream {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn a_wrapped_public_error_stays_hidden() {
        let wrapped = Upstream(PublicRpcError::new(1001, "secret detail"));
        assert_eq!(outcome(Box::new(wrapped)), HandlerErrorOutcome::Hidden);
        let plain = std::io::Error::other("password=hunter2");
        assert_eq!(outcome(Box::new(plain)), HandlerErrorOutcome::Hidden);
    }
}
