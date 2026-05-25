//! Validation traits and utilities for OpenRPC specification compliance.

use crate::error::{OpenRpcError, OpenRpcResult};
use std::collections::HashSet;

/// Trait for validating OpenRPC specification objects.
pub trait Validate {
    /// Validate this object against OpenRPC specification constraints.
    ///
    /// Returns `Ok(())` if valid, or an `OpenRpcError` describing the validation failure.
    fn validate(&self) -> OpenRpcResult<()>;
}

/// Trait for validating collections with uniqueness constraints.
pub trait ValidateUnique<T> {
    /// Validate that all items in this collection have unique keys according to the provided key function.
    fn validate_unique<K, F>(&self, key_fn: F, context: &str) -> OpenRpcResult<()>
    where
        K: std::hash::Hash + Eq + Clone + std::fmt::Display,
        F: Fn(&T) -> K;
}

impl<T> ValidateUnique<T> for Vec<T> {
    fn validate_unique<K, F>(&self, key_fn: F, context: &str) -> OpenRpcResult<()>
    where
        K: std::hash::Hash + Eq + Clone + std::fmt::Display,
        F: Fn(&T) -> K,
    {
        let mut seen = HashSet::new();
        for item in self {
            let key = key_fn(item);
            if !seen.insert(key.clone()) {
                return Err(OpenRpcError::duplicate_key(key.to_string(), context));
            }
        }
        Ok(())
    }
}

/// Validate URL format
pub fn validate_url(url: &str) -> OpenRpcResult<()> {
    // Basic URL validation - checks for scheme presence
    if url.is_empty() {
        return Err(OpenRpcError::invalid_url("URL cannot be empty"));
    }

    // Check if it looks like a URL (has scheme or is relative)
    if !url.contains("://") && !url.starts_with('/') && !url.starts_with("localhost") {
        return Err(OpenRpcError::invalid_url(format!(
            "Invalid URL format: {}",
            url
        )));
    }

    Ok(())
}

/// Validate email format
pub fn validate_email(email: &str) -> OpenRpcResult<()> {
    if email.is_empty() {
        return Err(OpenRpcError::invalid_email("Email cannot be empty"));
    }

    // Basic email validation - must contain @ and domain part
    if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        return Err(OpenRpcError::invalid_email(format!(
            "Invalid email format: {}",
            email
        )));
    }

    let parts: Vec<&str> = email.splitn(2, '@').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(OpenRpcError::invalid_email(format!(
            "Invalid email format: {}",
            email
        )));
    }

    // Domain must contain at least one dot
    if !parts[1].contains('.') {
        return Err(OpenRpcError::invalid_email(format!(
            "Invalid email domain: {}",
            email
        )));
    }

    Ok(())
}

/// Validate semver version format
pub fn validate_semver(version: &str) -> OpenRpcResult<()> {
    if version.is_empty() {
        return Err(OpenRpcError::validation("Version cannot be empty"));
    }

    // Basic semver pattern check: major.minor.patch
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 2 {
        return Err(OpenRpcError::validation(format!(
            "Invalid version format: {}",
            version
        )));
    }

    // Check that major and minor are numeric
    for (i, part) in parts.iter().take(2).enumerate() {
        if part.parse::<u32>().is_err() {
            let component = if i == 0 { "major" } else { "minor" };
            return Err(OpenRpcError::validation(format!(
                "Invalid {} version component: {}",
                component, part
            )));
        }
    }

    Ok(())
}

/// Validate OpenRPC specification version
pub fn validate_openrpc_version(version: &str) -> OpenRpcResult<()> {
    validate_semver(version)?;

    if !crate::version::is_supported(version) {
        return Err(OpenRpcError::unsupported_version(version));
    }

    Ok(())
}

/// Validate that a string contains only valid key characters for components
pub fn validate_component_key(key: &str) -> OpenRpcResult<()> {
    if key.is_empty() {
        return Err(OpenRpcError::validation("Component key cannot be empty"));
    }

    // OpenRPC spec: ^[a-zA-Z0-9\.\-_]+$
    for ch in key.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '.' && ch != '-' && ch != '_' {
            return Err(OpenRpcError::validation(format!(
                "Invalid component key character '{}' in key '{}'",
                ch, key
            )));
        }
    }

    Ok(())
}

/// Validate JSON-RPC error code (must be integer, certain ranges reserved)
pub fn validate_error_code(code: i64) -> OpenRpcResult<()> {
    // Pre-defined error codes are reserved: -32768 to -32000
    if (-32768..=-32000).contains(&code) {
        return Err(OpenRpcError::validation(format!(
            "Error code {} is in reserved range (-32768 to -32000)",
            code
        )));
    }

    Ok(())
}

/// Validate parameter structure enum value
pub fn validate_param_structure(param_structure: &str) -> OpenRpcResult<()> {
    match param_structure {
        "by-name" | "by-position" | "either" => Ok(()),
        _ => Err(OpenRpcError::validation(format!(
            "Invalid paramStructure value: '{}'. Must be one of: by-name, by-position, either",
            param_structure
        ))),
    }
}

/// Validate method name (must be unique within methods array)
pub fn validate_method_name(name: &str) -> OpenRpcResult<()> {
    if name.is_empty() {
        return Err(OpenRpcError::validation("Method name cannot be empty"));
    }

    // Method names should be valid JSON-RPC method names
    // Basic validation - no spaces, not starting with rpc. unless it's rpc.discover
    if name.contains(' ') {
        return Err(OpenRpcError::validation(format!(
            "Method name '{}' cannot contain spaces",
            name
        )));
    }

    if name.starts_with("rpc.") && name != "rpc.discover" {
        return Err(OpenRpcError::validation(format!(
            "Method name '{}' uses reserved 'rpc.' prefix",
            name
        )));
    }

    Ok(())
}

/// Validate content descriptor name (must be unique within params array when by-name)
pub fn validate_content_descriptor_name(name: &str) -> OpenRpcResult<()> {
    if name.is_empty() {
        return Err(OpenRpcError::validation(
            "Content descriptor name cannot be empty",
        ));
    }

    // Names should be valid parameter names (no spaces, valid identifier-like)
    if name.contains(' ') {
        return Err(OpenRpcError::validation(format!(
            "Content descriptor name '{}' cannot contain spaces",
            name
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("/relative/path").is_ok());
        assert!(validate_url("localhost").is_ok());

        assert!(validate_url("").is_err());
        assert!(validate_url("not-a-url").is_err());
    }

    #[test]
    fn test_validate_email() {
        assert!(validate_email("test@example.com").is_ok());
        assert!(validate_email("user.name@domain.co.uk").is_ok());

        assert!(validate_email("").is_err());
        assert!(validate_email("invalid").is_err());
        assert!(validate_email("@example.com").is_err());
        assert!(validate_email("test@").is_err());
        assert!(validate_email("test@invalid").is_err());
    }

    #[test]
    fn test_validate_semver() {
        assert!(validate_semver("1.0.0").is_ok());
        assert!(validate_semver("1.3.2").is_ok());
        assert!(validate_semver("0.1").is_ok());

        assert!(validate_semver("").is_err());
        assert!(validate_semver("1").is_err());
        assert!(validate_semver("v1.0.0").is_err());
        assert!(validate_semver("1.a.0").is_err());
    }

    #[test]
    fn test_validate_openrpc_version() {
        assert!(validate_openrpc_version("1.3.2").is_ok());
        assert!(validate_openrpc_version("1.0.0").is_ok());

        assert!(validate_openrpc_version("2.0.0").is_err());
        assert!(validate_openrpc_version("0.9.0").is_err());
    }

    #[test]
    fn test_validate_component_key() {
        assert!(validate_component_key("validKey").is_ok());
        assert!(validate_component_key("valid.key").is_ok());
        assert!(validate_component_key("valid-key").is_ok());
        assert!(validate_component_key("valid_key").is_ok());
        assert!(validate_component_key("123").is_ok());

        assert!(validate_component_key("").is_err());
        assert!(validate_component_key("invalid key").is_err());
        assert!(validate_component_key("invalid$key").is_err());
    }

    #[test]
    fn test_validate_error_code() {
        assert!(validate_error_code(-32001).is_err()); // Reserved range
        assert!(validate_error_code(-32768).is_err()); // Reserved range
        assert!(validate_error_code(-32000).is_err()); // Reserved range

        assert!(validate_error_code(-31999).is_ok()); // Outside reserved range
        assert!(validate_error_code(1000).is_ok()); // Positive codes
        assert!(validate_error_code(-1).is_ok()); // Negative but not reserved
    }

    #[test]
    fn test_validate_param_structure() {
        assert!(validate_param_structure("by-name").is_ok());
        assert!(validate_param_structure("by-position").is_ok());
        assert!(validate_param_structure("either").is_ok());

        assert!(validate_param_structure("invalid").is_err());
        assert!(validate_param_structure("").is_err());
    }

    #[test]
    fn test_validate_method_name() {
        assert!(validate_method_name("validMethod").is_ok());
        assert!(validate_method_name("rpc.discover").is_ok());
        assert!(validate_method_name("snake_case").is_ok());

        assert!(validate_method_name("").is_err());
        assert!(validate_method_name("method with spaces").is_err());
        assert!(validate_method_name("rpc.custom").is_err());
    }

    #[test]
    fn test_validate_unique() {
        let items = vec!["a", "b", "c"];
        assert!(items.validate_unique(|s| *s, "test context").is_ok());

        let items = vec!["a", "b", "a"];
        assert!(items.validate_unique(|s| *s, "test context").is_err());
    }
}
