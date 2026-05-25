//! JSON-RPC 2.0 protocol types and utilities.
//!
//! This crate provides type-safe representations of JSON-RPC 2.0 protocol
//! structures including requests, responses, and errors.

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// The JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// The method name to call.
    pub method: String,

    /// Parameters for the method call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,

    /// Request identifier for matching responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// The JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// The result of the method call (present on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// Error information (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,

    /// Request identifier for matching with requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 error structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code indicating the type of error.
    pub code: i32,

    /// Human-readable error message.
    pub message: String,

    /// Additional error information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Standard JSON-RPC error codes.
pub mod error_codes {
    /// Invalid JSON was received by the server.
    pub const PARSE_ERROR: i32 = -32700;

    /// The JSON sent is not a valid Request object.
    pub const INVALID_REQUEST: i32 = -32600;

    /// The method does not exist / is not available.
    pub const METHOD_NOT_FOUND: i32 = -32601;

    /// Invalid method parameter(s).
    pub const INVALID_PARAMS: i32 = -32602;

    /// Internal JSON-RPC error.
    pub const INTERNAL_ERROR: i32 = -32603;

    /// Authentication required.
    pub const AUTHENTICATION_REQUIRED: i32 = -32001;

    /// Insufficient permissions.
    pub const INSUFFICIENT_PERMISSIONS: i32 = -32002;

    /// Token expired.
    pub const TOKEN_EXPIRED: i32 = -32003;

    /// CSRF validation failed.
    pub const CSRF_VALIDATION_FAILED: i32 = -32004;
}

impl JsonRpcRequest {
    /// Creates a new JSON-RPC request.
    pub fn new(
        method: String,
        params: Option<serde_json::Value>,
        id: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
            id,
        }
    }
}

impl JsonRpcResponse {
    /// Creates a successful JSON-RPC response.
    pub fn success(result: serde_json::Value, id: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Creates an error JSON-RPC response.
    pub fn error(error: JsonRpcError, id: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

impl JsonRpcError {
    /// Creates a new JSON-RPC error.
    pub fn new(code: i32, message: String, data: Option<serde_json::Value>) -> Self {
        Self {
            code,
            message,
            data,
        }
    }

    /// Creates a parse error.
    pub fn parse_error() -> Self {
        Self::new(error_codes::PARSE_ERROR, "Parse error".to_string(), None)
    }

    /// Creates an invalid request error.
    pub fn invalid_request() -> Self {
        Self::new(
            error_codes::INVALID_REQUEST,
            "Invalid Request".to_string(),
            None,
        )
    }

    /// Creates a method not found error.
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", method),
            None,
        )
    }

    /// Creates an invalid params error.
    pub fn invalid_params(_details: String) -> Self {
        Self::new(
            error_codes::INVALID_PARAMS,
            "Invalid params".to_string(),
            None,
        )
    }

    /// Creates an internal error.
    pub fn internal_error(_details: String) -> Self {
        Self::new(
            error_codes::INTERNAL_ERROR,
            "Internal error".to_string(),
            None,
        )
    }

    /// Creates an authentication required error.
    pub fn authentication_required() -> Self {
        Self::new(
            error_codes::AUTHENTICATION_REQUIRED,
            "Authentication required".to_string(),
            None,
        )
    }

    /// Creates an insufficient permissions error.
    pub fn insufficient_permissions(required: Vec<String>, has: Vec<String>) -> Self {
        Self::new(
            error_codes::INSUFFICIENT_PERMISSIONS,
            "Insufficient permissions".to_string(),
            Some(serde_json::json!({
                "required": required,
                "has": has
            })),
        )
    }

    /// Creates a token expired error.
    pub fn token_expired() -> Self {
        Self::new(
            error_codes::TOKEN_EXPIRED,
            "Token expired".to_string(),
            None,
        )
    }

    /// Creates a CSRF validation error.
    pub fn csrf_validation_failed() -> Self {
        Self::new(
            error_codes::CSRF_VALIDATION_FAILED,
            "CSRF validation failed".to_string(),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_request_constructor_sets_version() {
        let r = JsonRpcRequest::new(
            "m".into(),
            Some(serde_json::json!(1)),
            Some(serde_json::json!("rid")),
        );
        assert_eq!(r.jsonrpc, "2.0");
        assert_eq!(r.method, "m");
    }

    #[test]
    fn jsonrpc_response_success_and_error() {
        let s = JsonRpcResponse::success(serde_json::json!("ok"), Some(serde_json::json!(1)));
        assert_eq!(s.jsonrpc, "2.0");
        assert!(s.error.is_none());
        assert_eq!(s.result, Some(serde_json::json!("ok")));

        let e = JsonRpcResponse::error(JsonRpcError::parse_error(), Some(serde_json::json!(1)));
        assert!(e.result.is_none());
        assert_eq!(e.error.unwrap().code, error_codes::PARSE_ERROR);
    }

    #[test]
    fn json_rpc_error_constructors_use_canonical_codes() {
        assert_eq!(JsonRpcError::parse_error().code, error_codes::PARSE_ERROR);
        assert_eq!(
            JsonRpcError::invalid_request().code,
            error_codes::INVALID_REQUEST
        );
        let nf = JsonRpcError::method_not_found("m");
        assert_eq!(nf.code, error_codes::METHOD_NOT_FOUND);
        assert!(nf.message.contains("m"));
        assert_eq!(
            JsonRpcError::invalid_params("bad".into()).code,
            error_codes::INVALID_PARAMS
        );
        assert_eq!(
            JsonRpcError::internal_error("e".into()).code,
            error_codes::INTERNAL_ERROR
        );
        assert_eq!(
            JsonRpcError::authentication_required().code,
            error_codes::AUTHENTICATION_REQUIRED
        );
        assert_eq!(
            JsonRpcError::token_expired().code,
            error_codes::TOKEN_EXPIRED
        );
        assert_eq!(
            JsonRpcError::csrf_validation_failed().code,
            error_codes::CSRF_VALIDATION_FAILED
        );
    }

    #[test]
    fn insufficient_permissions_carries_data() {
        let err = JsonRpcError::insufficient_permissions(vec!["admin".into()], vec!["user".into()]);
        assert_eq!(err.code, error_codes::INSUFFICIENT_PERMISSIONS);
        let data = err.data.unwrap();
        assert_eq!(data["required"], serde_json::json!(["admin"]));
        assert_eq!(data["has"], serde_json::json!(["user"]));
    }

    #[test]
    fn request_with_no_id_skips_field_in_serialization() {
        let req = JsonRpcRequest::new("notify".into(), None, None);
        let s = serde_json::to_string(&req).unwrap();
        assert!(!s.contains("\"id\""));
        assert!(!s.contains("\"params\""));
    }

    #[test]
    fn request_serializes_canonical_jsonrpc_wire_shape() {
        let request = JsonRpcRequest::new(
            "subtract".to_string(),
            Some(serde_json::json!([42, 23])),
            Some(serde_json::json!(1)),
        );

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "subtract",
                "params": [42, 23],
                "id": 1
            })
        );
    }

    #[test]
    fn success_response_omits_error_field() {
        let response = JsonRpcResponse::success(
            serde_json::json!({ "value": 19 }),
            Some(serde_json::json!("req-1")),
        );

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": { "value": 19 },
                "id": "req-1"
            })
        );
    }

    #[test]
    fn error_response_omits_result_field_and_keeps_error_data() {
        let response = JsonRpcResponse::error(
            JsonRpcError::new(
                error_codes::INVALID_PARAMS,
                "Invalid params".to_string(),
                Some(serde_json::json!({ "field": "name" })),
            ),
            Some(serde_json::json!("req-2")),
        );

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32602,
                    "message": "Invalid params",
                    "data": { "field": "name" }
                },
                "id": "req-2"
            })
        );
    }
}
