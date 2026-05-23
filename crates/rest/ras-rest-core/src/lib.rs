//! Core types and traits for REST services in Rust Agent Stack.
//!
//! This crate provides the runtime types needed for REST services, including:
//! - `RestResult`, `RestResponse`, and `RestError` for explicit HTTP status code handling
//! - Re-exports of authentication types from `ras-auth-core`

use thiserror::Error;

// Re-export authentication types for convenience
pub use ras_auth_core::{AuthError, AuthProvider, AuthResult, AuthenticatedUser};
pub use ras_version_core::*;

/// Result type for REST handlers that allows explicit HTTP status codes.
pub type RestResult<T> = Result<RestResponse<T>, RestError>;

/// Successful REST response wrapper.
#[derive(Debug, Clone)]
pub struct RestResponse<T> {
    /// HTTP status code (default: 200)
    pub status: u16,
    /// Response body
    pub body: T,
}

impl<T> RestResponse<T> {
    /// Create a 200 OK response.
    pub fn ok(body: T) -> Self {
        Self { status: 200, body }
    }

    /// Create a 201 Created response.
    pub fn created(body: T) -> Self {
        Self { status: 201, body }
    }

    /// Create a 202 Accepted response.
    pub fn accepted(body: T) -> Self {
        Self { status: 202, body }
    }

    /// Create a 204 No Content response (requires T to be ()).
    pub fn no_content() -> Self
    where
        T: Default,
    {
        Self {
            status: 204,
            body: T::default(),
        }
    }

    /// Create a response with a custom status code.
    pub fn with_status(status: u16, body: T) -> Self {
        Self { status, body }
    }
}

/// REST error with explicit HTTP status code.
#[derive(Debug, Error)]
#[error("HTTP {status}: {message}")]
pub struct RestError {
    /// HTTP status code
    pub status: u16,
    /// Error message to send to client
    pub message: String,
    /// Optional internal error for logging (not sent to client)
    #[source]
    pub internal_error: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl RestError {
    /// Create a new REST error.
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            internal_error: None,
        }
    }

    /// Create a new REST error with internal error details for logging.
    pub fn with_internal<E>(status: u16, message: impl Into<String>, internal: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self {
            status,
            message: message.into(),
            internal_error: Some(Box::new(internal)),
        }
    }

    /// Create a 400 Bad Request error.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, message)
    }

    /// Create a 401 Unauthorized error.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(401, message)
    }

    /// Create a 403 Forbidden error.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(403, message)
    }

    /// Create a 404 Not Found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(404, message)
    }

    /// Create a 409 Conflict error.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(409, message)
    }

    /// Create a 422 Unprocessable Entity error.
    pub fn unprocessable_entity(message: impl Into<String>) -> Self {
        Self::new(422, message)
    }

    /// Create a 500 Internal Server Error.
    pub fn internal_server_error(message: impl Into<String>) -> Self {
        Self::new(500, message)
    }

    /// Create a 502 Bad Gateway error.
    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(502, message)
    }

    /// Create a 503 Service Unavailable error.
    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(503, message)
    }
}

/// Helper trait to convert various error types to RestError.
pub trait IntoRestError {
    /// Convert this error into a RestError.
    fn into_rest_error(self) -> RestError;
}

impl<E: std::error::Error + Send + Sync + 'static> IntoRestError for E {
    fn into_rest_error(self) -> RestError {
        RestError::with_internal(500, "Internal server error", self)
    }
}

/// Extension trait for Result types to easily convert errors to RestError.
pub trait RestResultExt<T> {
    /// Convert any error to a RestError with a 500 status code.
    fn internal_server_error(self) -> RestResult<T>;

    /// Convert any error to a RestError with a custom status code and message.
    fn rest_error(self, status: u16, message: impl Into<String>) -> RestResult<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> RestResultExt<T> for Result<T, E> {
    fn internal_server_error(self) -> RestResult<T> {
        self.map(RestResponse::ok)
            .map_err(|e| RestError::with_internal(500, "Internal server error", e))
    }

    fn rest_error(self, status: u16, message: impl Into<String>) -> RestResult<T> {
        let msg = message.into();
        self.map(RestResponse::ok)
            .map_err(|e| RestError::with_internal(status, msg, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_response_constructors_set_correct_status() {
        assert_eq!(RestResponse::ok(1).status, 200);
        assert_eq!(RestResponse::created("x").status, 201);
        assert_eq!(RestResponse::accepted(true).status, 202);
        let nc: RestResponse<()> = RestResponse::no_content();
        assert_eq!(nc.status, 204);
        assert_eq!(RestResponse::with_status(418, "tea").status, 418);
        // Body is preserved.
        assert_eq!(RestResponse::ok(42).body, 42);
    }

    #[test]
    fn rest_error_constructors_set_correct_status_and_message() {
        let cases = [
            (RestError::bad_request("a"), 400),
            (RestError::unauthorized("a"), 401),
            (RestError::forbidden("a"), 403),
            (RestError::not_found("a"), 404),
            (RestError::conflict("a"), 409),
            (RestError::unprocessable_entity("a"), 422),
            (RestError::internal_server_error("a"), 500),
            (RestError::bad_gateway("a"), 502),
            (RestError::service_unavailable("a"), 503),
        ];
        for (err, expected) in cases {
            assert_eq!(err.status, expected);
            assert_eq!(err.message, "a");
            assert!(err.internal_error.is_none());
            // Display includes the status and message.
            let s = err.to_string();
            assert!(s.contains(&expected.to_string()));
            assert!(s.contains("a"));
        }
    }

    #[test]
    fn rest_error_with_internal_carries_source() {
        #[derive(Debug, thiserror::Error)]
        #[error("inner failure")]
        struct Inner;
        let err = RestError::with_internal(503, "down", Inner);
        assert_eq!(err.status, 503);
        assert!(err.internal_error.is_some());
        // source() returns the wrapped error.
        let src = std::error::Error::source(&err).unwrap();
        assert_eq!(src.to_string(), "inner failure");
    }

    #[test]
    fn rest_error_new_has_stable_display_and_no_source() {
        let err = RestError::new(429, "too many requests");

        assert_eq!(err.status, 429);
        assert_eq!(err.message, "too many requests");
        assert!(std::error::Error::source(&err).is_none());
        assert_eq!(err.to_string(), "HTTP 429: too many requests");
    }

    #[test]
    fn into_rest_error_blanket_impl() {
        let err = std::io::Error::other("io");
        let rest = err.into_rest_error();
        assert_eq!(rest.status, 500);
        assert_eq!(rest.message, "Internal server error");
        assert!(rest.internal_error.is_some());
    }

    #[test]
    fn rest_result_ext_maps_ok_and_err() {
        let ok: Result<i32, std::io::Error> = Ok(7);
        let mapped: RestResult<i32> = ok.internal_server_error();
        let resp = mapped.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, 7);

        let err: Result<i32, std::io::Error> = Err(std::io::Error::other("x"));
        let mapped: RestResult<i32> = err.internal_server_error();
        let e = mapped.unwrap_err();
        assert_eq!(e.status, 500);

        // rest_error variant lets callers customize.
        let err: Result<i32, std::io::Error> = Err(std::io::Error::other("x"));
        let mapped: RestResult<i32> = err.rest_error(418, "teapot");
        let e = mapped.unwrap_err();
        assert_eq!(e.status, 418);
        assert_eq!(e.message, "teapot");
    }

    #[test]
    fn rest_result_ext_custom_error_preserves_internal_source() {
        let err: Result<i32, std::io::Error> = Err(std::io::Error::other("database down"));

        let mapped = err.rest_error(503, "service unavailable").unwrap_err();

        assert_eq!(mapped.status, 503);
        assert_eq!(mapped.message, "service unavailable");
        assert_eq!(
            std::error::Error::source(&mapped)
                .expect("source")
                .to_string(),
            "database down"
        );
    }
}
