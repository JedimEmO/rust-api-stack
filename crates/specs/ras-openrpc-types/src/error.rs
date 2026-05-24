//! Error types for OpenRPC specification validation and processing.

use std::fmt;

/// Errors that can occur when working with OpenRPC specifications.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenRpcError {
    /// Validation error when OpenRPC specification constraints are violated
    ValidationError {
        /// Human-readable error message
        message: String,
        /// Optional field path where the error occurred
        field_path: Option<String>,
    },

    /// Error when parsing or serializing JSON
    JsonError {
        /// JSON parsing/serialization error message
        message: String,
    },

    /// Error when resolving references ($ref)
    ReferenceError {
        /// Reference resolution error message
        message: String,
        /// The reference string that failed to resolve
        reference: String,
    },

    /// Error when a required field is missing
    MissingField {
        /// Name of the missing required field
        field_name: String,
    },

    /// Error when a field has an invalid value
    InvalidField {
        /// Name of the field with invalid value
        field_name: String,
        /// Description of why the value is invalid
        message: String,
    },

    /// Error when an object has duplicate keys that should be unique
    DuplicateKey {
        /// The duplicate key name
        key: String,
        /// Context where the duplicate was found
        context: String,
    },

    /// Error when URL format is invalid
    InvalidUrl {
        /// The invalid URL string
        url: String,
    },

    /// Error when email format is invalid
    InvalidEmail {
        /// The invalid email string
        email: String,
    },

    /// Error when regex pattern is invalid
    InvalidRegex {
        /// The invalid regex pattern
        pattern: String,
    },

    /// Error when OpenRPC version is unsupported
    UnsupportedVersion {
        /// The unsupported version string
        version: String,
    },

    /// Error when JSON Schema Draft 7 constraints are violated
    SchemaError {
        /// Schema validation error message
        message: String,
        /// Optional schema path where the error occurred
        schema_path: Option<String>,
    },
}

impl fmt::Display for OpenRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValidationError {
                message,
                field_path: Some(field_path),
            } => write!(f, "Validation error at {field_path}: {message}"),
            Self::ValidationError {
                message,
                field_path: None,
            } => write!(f, "Validation error: {message}"),
            Self::JsonError { message } => write!(f, "JSON error: {message}"),
            Self::ReferenceError { message, .. } => {
                write!(f, "Reference resolution error: {message}")
            }
            Self::MissingField { field_name } => {
                write!(f, "Missing required field: {field_name}")
            }
            Self::InvalidField {
                field_name,
                message,
            } => write!(f, "Invalid field value for '{field_name}': {message}"),
            Self::DuplicateKey { key, context } => {
                write!(f, "Duplicate key '{key}' found in {context}")
            }
            Self::InvalidUrl { url } => write!(f, "Invalid URL format: {url}"),
            Self::InvalidEmail { email } => write!(f, "Invalid email format: {email}"),
            Self::InvalidRegex { pattern } => write!(f, "Invalid regex pattern: {pattern}"),
            Self::UnsupportedVersion { version } => {
                write!(f, "Unsupported OpenRPC version: {version}")
            }
            Self::SchemaError {
                message,
                schema_path: Some(schema_path),
            } => write!(
                f,
                "JSON Schema validation error at {schema_path}: {message}"
            ),
            Self::SchemaError {
                message,
                schema_path: None,
            } => write!(f, "JSON Schema validation error: {message}"),
        }
    }
}

impl std::error::Error for OpenRpcError {}

impl OpenRpcError {
    /// Create a new validation error
    pub fn validation(message: impl Into<String>) -> Self {
        Self::ValidationError {
            message: message.into(),
            field_path: None,
        }
    }

    /// Create a new validation error with field path
    pub fn validation_with_path(message: impl Into<String>, field_path: impl Into<String>) -> Self {
        Self::ValidationError {
            message: message.into(),
            field_path: Some(field_path.into()),
        }
    }

    /// Create a new JSON error
    pub fn json(message: impl Into<String>) -> Self {
        Self::JsonError {
            message: message.into(),
        }
    }

    /// Create a new reference resolution error
    pub fn reference(message: impl Into<String>, reference: impl Into<String>) -> Self {
        Self::ReferenceError {
            message: message.into(),
            reference: reference.into(),
        }
    }

    /// Create a new missing field error
    pub fn missing_field(field_name: impl Into<String>) -> Self {
        Self::MissingField {
            field_name: field_name.into(),
        }
    }

    /// Create a new invalid field error
    pub fn invalid_field(field_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidField {
            field_name: field_name.into(),
            message: message.into(),
        }
    }

    /// Create a new duplicate key error
    pub fn duplicate_key(key: impl Into<String>, context: impl Into<String>) -> Self {
        Self::DuplicateKey {
            key: key.into(),
            context: context.into(),
        }
    }

    /// Create a new invalid URL error
    pub fn invalid_url(url: impl Into<String>) -> Self {
        Self::InvalidUrl { url: url.into() }
    }

    /// Create a new invalid email error
    pub fn invalid_email(email: impl Into<String>) -> Self {
        Self::InvalidEmail {
            email: email.into(),
        }
    }

    /// Create a new invalid regex error
    pub fn invalid_regex(pattern: impl Into<String>) -> Self {
        Self::InvalidRegex {
            pattern: pattern.into(),
        }
    }

    /// Create a new unsupported version error
    pub fn unsupported_version(version: impl Into<String>) -> Self {
        Self::UnsupportedVersion {
            version: version.into(),
        }
    }

    /// Create a new schema validation error
    pub fn schema(message: impl Into<String>) -> Self {
        Self::SchemaError {
            message: message.into(),
            schema_path: None,
        }
    }

    /// Create a new schema validation error with path
    pub fn schema_with_path(message: impl Into<String>, schema_path: impl Into<String>) -> Self {
        Self::SchemaError {
            message: message.into(),
            schema_path: Some(schema_path.into()),
        }
    }
}

impl From<serde_json::Error> for OpenRpcError {
    fn from(err: serde_json::Error) -> Self {
        Self::json(err.to_string())
    }
}

/// Result type for OpenRPC operations
pub type OpenRpcResult<T> = Result<T, OpenRpcError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = OpenRpcError::validation("test message");
        assert!(matches!(err, OpenRpcError::ValidationError { .. }));

        let err = OpenRpcError::validation_with_path("test", "field.path");
        if let OpenRpcError::ValidationError { field_path, .. } = err {
            assert_eq!(field_path, Some("field.path".to_string()));
        } else {
            panic!("Expected ValidationError");
        }
    }

    #[test]
    fn test_error_display() {
        let err = OpenRpcError::validation("test validation error");
        assert_eq!(err.to_string(), "Validation error: test validation error");

        let err = OpenRpcError::validation_with_path("nested failure", "methods[0].params[1]");
        assert_eq!(
            err.to_string(),
            "Validation error at methods[0].params[1]: nested failure"
        );

        let err = OpenRpcError::schema_with_path("expected string", "properties.name");
        assert_eq!(
            err.to_string(),
            "JSON Schema validation error at properties.name: expected string"
        );

        let err = OpenRpcError::missing_field("required_field");
        assert_eq!(err.to_string(), "Missing required field: required_field");
    }

    #[test]
    fn display_messages_cover_all_error_variants() {
        let cases = [
            (
                OpenRpcError::json("unexpected token"),
                "JSON error: unexpected token",
            ),
            (
                OpenRpcError::reference("not found", "#/components/schemas/Missing"),
                "Reference resolution error: not found",
            ),
            (
                OpenRpcError::invalid_field("name", "must not be blank"),
                "Invalid field value for 'name': must not be blank",
            ),
            (
                OpenRpcError::duplicate_key("id", "method parameters"),
                "Duplicate key 'id' found in method parameters",
            ),
            (
                OpenRpcError::invalid_url("not-a-url"),
                "Invalid URL format: not-a-url",
            ),
            (
                OpenRpcError::invalid_email("not-an-email"),
                "Invalid email format: not-an-email",
            ),
            (OpenRpcError::invalid_regex("["), "Invalid regex pattern: ["),
            (
                OpenRpcError::unsupported_version("2.0.0"),
                "Unsupported OpenRPC version: 2.0.0",
            ),
            (
                OpenRpcError::schema("invalid schema"),
                "JSON Schema validation error: invalid schema",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn reference_error_keeps_reference_for_callers() {
        let error = OpenRpcError::reference("not found", "#/components/schemas/Missing");

        assert_eq!(
            error,
            OpenRpcError::ReferenceError {
                message: "not found".to_string(),
                reference: "#/components/schemas/Missing".to_string(),
            }
        );
    }

    #[test]
    fn test_json_error_conversion() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json");
        assert!(json_err.is_err());

        let openrpc_err: OpenRpcError = json_err.unwrap_err().into();
        assert!(matches!(openrpc_err, OpenRpcError::JsonError { .. }));
    }
}
