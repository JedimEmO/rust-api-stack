//! Error Object for OpenRPC specification.

use crate::{Extensions, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Defines an application level error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct ErrorObject {
    /// A Number that indicates the error type that occurred.
    /// This MUST be an integer. The error codes from and including -32768 to -32000
    /// are reserved for pre-defined errors. These pre-defined errors SHOULD be assumed
    /// to be returned from any JSON-RPC api.
    pub code: i64,

    /// A String providing a short description of the error.
    /// The message SHOULD be limited to a concise single sentence.
    pub message: String,

    /// A Primitive or Structured value that contains additional information about the error.
    /// This may be omitted. The value of this member is defined by the Server
    /// (e.g. detailed error information, nested errors etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl ErrorObject {
    /// Create a new ErrorObject with required fields
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the data field
    pub fn with_data(mut self, data: impl Into<Value>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Add an extension field
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key, value);
        self
    }

    /// Check if this error code is in the reserved range
    pub fn is_reserved_code(&self) -> bool {
        (-32768..=-32000).contains(&self.code)
    }

    /// Create a parse error (-32700)
    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    /// Create an invalid request error (-32600)
    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid Request")
    }

    /// Create a method not found error (-32601)
    pub fn method_not_found() -> Self {
        Self::new(-32601, "Method not found")
    }

    /// Create an invalid params error (-32602)
    pub fn invalid_params() -> Self {
        Self::new(-32602, "Invalid params")
    }

    /// Create an internal error (-32603)
    pub fn internal_error() -> Self {
        Self::new(-32603, "Internal error")
    }

    /// Create a server error (range -32099 to -32000)
    pub fn server_error(
        code: i64,
        message: impl Into<String>,
    ) -> Result<Self, crate::error::OpenRpcError> {
        if !(-32099..=-32000).contains(&code) {
            return Err(crate::error::OpenRpcError::validation(format!(
                "Server error code must be in range -32099 to -32000, got {}",
                code
            )));
        }
        Ok(Self::new(code, message))
    }

    /// Create an application-defined error (outside reserved range)
    pub fn application_error(
        code: i64,
        message: impl Into<String>,
    ) -> Result<Self, crate::error::OpenRpcError> {
        if (-32768..=-32000).contains(&code) {
            return Err(crate::error::OpenRpcError::validation(format!(
                "Application error code {} is in reserved range (-32768 to -32000)",
                code
            )));
        }
        Ok(Self::new(code, message))
    }
}

impl Validate for ErrorObject {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate message
        if self.message.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("message"));
        }

        // Validate error code if it's not in reserved range
        if !self.is_reserved_code() {
            crate::validation::validate_error_code(self.code)?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

/// Pre-defined JSON-RPC error codes
pub mod error_codes {
    /// Parse error - Invalid JSON was received by the server
    pub const PARSE_ERROR: i64 = -32700;

    /// Invalid Request - The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i64 = -32600;

    /// Method not found - The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i64 = -32601;

    /// Invalid params - Invalid method parameter(s)
    pub const INVALID_PARAMS: i64 = -32602;

    /// Internal error - Internal JSON-RPC error
    pub const INTERNAL_ERROR: i64 = -32603;

    /// Server error range start
    pub const SERVER_ERROR_MIN: i64 = -32099;

    /// Server error range end
    pub const SERVER_ERROR_MAX: i64 = -32000;

    /// Reserved error range start
    pub const RESERVED_MIN: i64 = -32768;

    /// Reserved error range end
    pub const RESERVED_MAX: i64 = -32000;

    /// Check if an error code is reserved
    pub fn is_reserved(code: i64) -> bool {
        (RESERVED_MIN..=RESERVED_MAX).contains(&code)
    }

    /// Check if an error code is a server error
    pub fn is_server_error(code: i64) -> bool {
        (SERVER_ERROR_MIN..=SERVER_ERROR_MAX).contains(&code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_error_object_creation() {
        let error =
            ErrorObject::new(1000, "Application error").with_data(json!({"details": "More info"}));

        assert_eq!(error.code, 1000);
        assert_eq!(error.message, "Application error");
        assert_eq!(error.data, Some(json!({"details": "More info"})));
    }

    #[test]
    fn test_predefined_errors() {
        let parse_error = ErrorObject::parse_error();
        assert_eq!(parse_error.code, -32700);
        assert_eq!(parse_error.message, "Parse error");

        let method_not_found = ErrorObject::method_not_found();
        assert_eq!(method_not_found.code, -32601);
        assert_eq!(method_not_found.message, "Method not found");
    }

    #[test]
    fn test_server_error() {
        let server_error = ErrorObject::server_error(-32001, "Custom server error").unwrap();
        assert_eq!(server_error.code, -32001);

        // Invalid server error code
        let result = ErrorObject::server_error(-31999, "Invalid code");
        assert!(result.is_err());
    }

    #[test]
    fn test_application_error() {
        let app_error = ErrorObject::application_error(1000, "App error").unwrap();
        assert_eq!(app_error.code, 1000);

        // Invalid application error code (in reserved range)
        let result = ErrorObject::application_error(-32001, "Reserved code");
        assert!(result.is_err());
    }

    #[test]
    fn test_error_validation() {
        // Valid error
        let error = ErrorObject::new(1000, "Valid error");
        assert!(error.validate().is_ok());

        // Invalid - empty message
        let error = ErrorObject::new(1000, "");
        assert!(error.validate().is_err());

        // Reserved codes are allowed in validation (they're pre-defined)
        let error = ErrorObject::parse_error();
        assert!(error.validate().is_ok());
    }

    #[test]
    fn test_reserved_code_check() {
        let error = ErrorObject::parse_error();
        assert!(error.is_reserved_code());

        let error = ErrorObject::new(1000, "App error");
        assert!(!error.is_reserved_code());
    }

    #[test]
    fn test_error_codes_module() {
        use error_codes::*;

        assert_eq!(PARSE_ERROR, -32700);
        assert_eq!(INVALID_REQUEST, -32600);
        assert_eq!(METHOD_NOT_FOUND, -32601);
        assert_eq!(INVALID_PARAMS, -32602);
        assert_eq!(INTERNAL_ERROR, -32603);

        assert!(is_reserved(PARSE_ERROR));
        assert!(is_server_error(-32001));
        assert!(!is_reserved(1000));
        assert!(!is_server_error(1000));
    }

    #[test]
    fn test_error_object_builder() {
        let error = ErrorObject::builder()
            .code(1000)
            .message("Test error".to_string())
            .data(json!({"key": "value"}))
            .build();

        assert_eq!(error.code, 1000);
        assert_eq!(error.message, "Test error");
        assert_eq!(error.data, Some(json!({"key": "value"})));
    }

    #[test]
    fn test_error_object_serialization() {
        let error = ErrorObject::new(1000, "Test error").with_data(json!({"details": "test"}));

        let json_value = serde_json::to_value(&error).unwrap();
        let expected = json!({
            "code": 1000,
            "message": "Test error",
            "data": {"details": "test"}
        });

        assert_eq!(json_value, expected);

        let deserialized: ErrorObject = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, error);
    }

    #[test]
    fn test_error_object_with_extensions() {
        let error = ErrorObject::new(1000, "Test").with_extension("x-custom", "value");

        assert!(!error.extensions.is_empty());
        assert_eq!(error.extensions.get("x-custom"), Some(&json!("value")));
    }
}
