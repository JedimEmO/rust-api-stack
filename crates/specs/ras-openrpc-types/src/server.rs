//! Server Object and Server Variable Object for OpenRPC specification.

use crate::{Extensions, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An object representing a Server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Server {
    /// A name to be used as the canonical name for the server.
    pub name: String,

    /// A URL to the target host. This URL supports Server Variables and MAY be relative,
    /// to indicate that the host location is relative to the location where the
    /// OpenRPC document is being served. Server Variables are passed into the
    /// Runtime Expression to produce a server URL.
    pub url: String,

    /// A short summary of what the server is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// An optional string describing the host designated by the URL.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A map between a variable name and its value. The value is passed into the
    /// Runtime Expression to produce a server URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, ServerVariable>>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Server {
    /// Create a new Server with required fields
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            summary: None,
            description: None,
            variables: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the summary
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the variables
    pub fn with_variables(mut self, variables: HashMap<String, ServerVariable>) -> Self {
        self.variables = Some(variables);
        self
    }

    /// Add a variable
    pub fn with_variable(mut self, name: impl Into<String>, variable: ServerVariable) -> Self {
        if self.variables.is_none() {
            self.variables = Some(HashMap::new());
        }
        self.variables
            .as_mut()
            .unwrap()
            .insert(name.into(), variable);
        self
    }

    /// Add an extension field
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key, value);
        self
    }

    /// Get the resolved URL by substituting variables
    pub fn resolve_url(&self) -> String {
        let mut resolved_url = self.url.clone();

        if let Some(ref variables) = self.variables {
            for (name, variable) in variables {
                let placeholder = format!("{{{}}}", name);
                if resolved_url.contains(&placeholder) {
                    resolved_url = resolved_url.replace(&placeholder, &variable.default);
                }
            }
        }

        resolved_url
    }
}

impl Validate for Server {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }

        // Validate URL
        if self.url.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("url"));
        }
        // Note: URL can contain template variables, so we don't validate format here

        // Validate variables
        if let Some(ref variables) = self.variables {
            for (name, variable) in variables {
                if name.is_empty() {
                    return Err(crate::error::OpenRpcError::validation(
                        "variable name cannot be empty",
                    ));
                }
                variable.validate()?;
            }
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

/// An object representing a Server Variable for server URL template substitution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct ServerVariable {
    /// An enumeration of string values to be used if the substitution options
    /// are from a limited set.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,

    /// The default value to use for substitution, which SHALL be sent if an
    /// alternate value is not supplied. Note this behavior is different than
    /// the Schema Object's treatment of default values, because in those cases
    /// parameter values are optional.
    pub default: String,

    /// An optional description for the server variable.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl ServerVariable {
    /// Create a new ServerVariable with required default value
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            enum_values: None,
            default: default.into(),
            description: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the enum values
    pub fn with_enum(mut self, enum_values: Vec<String>) -> Self {
        self.enum_values = Some(enum_values);
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add an extension field
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

impl Validate for ServerVariable {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate default value
        if self.default.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("default"));
        }

        // If enum is provided, default must be one of the enum values
        if let Some(ref enum_values) = self.enum_values {
            if enum_values.is_empty() {
                return Err(crate::error::OpenRpcError::validation(
                    "enum cannot be empty if provided",
                ));
            }

            if !enum_values.contains(&self.default) {
                return Err(crate::error::OpenRpcError::validation(format!(
                    "default value '{}' must be one of the enum values: {:?}",
                    self.default, enum_values
                )));
            }
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_server_creation() {
        let server = Server::new("production", "https://api.example.com")
            .with_summary("Production server")
            .with_description("Main production API server");

        assert_eq!(server.name, "production");
        assert_eq!(server.url, "https://api.example.com");
        assert_eq!(server.summary, Some("Production server".to_string()));
    }

    #[test]
    fn test_server_with_variables() {
        let port_var = ServerVariable::new("8080")
            .with_enum(vec!["8080".to_string(), "8443".to_string()])
            .with_description("Server port");

        let server =
            Server::new("test", "https://api.example.com:{port}").with_variable("port", port_var);

        assert!(server.variables.is_some());
        assert_eq!(server.variables.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_server_url_resolution() {
        let port_var = ServerVariable::new("8080");
        let version_var = ServerVariable::new("v1");

        let server = Server::new("test", "https://api.example.com:{port}/{version}")
            .with_variable("port", port_var)
            .with_variable("version", version_var);

        let resolved = server.resolve_url();
        assert_eq!(resolved, "https://api.example.com:8080/v1");
    }

    #[test]
    fn test_server_validation() {
        // Valid server
        let server = Server::new("test", "https://example.com");
        assert!(server.validate().is_ok());

        // Invalid - empty name
        let server = Server::new("", "https://example.com");
        assert!(server.validate().is_err());

        // Invalid - empty URL
        let server = Server::new("test", "");
        assert!(server.validate().is_err());
    }

    #[test]
    fn test_server_variable_creation() {
        let var = ServerVariable::new("production")
            .with_enum(vec!["production".to_string(), "staging".to_string()])
            .with_description("Environment");

        assert_eq!(var.default, "production");
        assert_eq!(
            var.enum_values,
            Some(vec!["production".to_string(), "staging".to_string()])
        );
    }

    #[test]
    fn test_server_variable_validation() {
        // Valid variable
        let var = ServerVariable::new("test");
        assert!(var.validate().is_ok());

        // Valid with enum
        let var =
            ServerVariable::new("prod").with_enum(vec!["prod".to_string(), "staging".to_string()]);
        assert!(var.validate().is_ok());

        // Invalid - empty default
        let var = ServerVariable::new("");
        assert!(var.validate().is_err());

        // Invalid - default not in enum
        let var = ServerVariable::new("invalid")
            .with_enum(vec!["prod".to_string(), "staging".to_string()]);
        assert!(var.validate().is_err());

        // Invalid - empty enum
        let var = ServerVariable::new("test").with_enum(vec![]);
        assert!(var.validate().is_err());
    }

    #[test]
    fn test_server_builder() {
        let server = Server::builder()
            .name("test".to_string())
            .url("https://example.com".to_string())
            .summary("Test server".to_string())
            .build();

        assert_eq!(server.name, "test");
        assert_eq!(server.url, "https://example.com");
        assert_eq!(server.summary, Some("Test server".to_string()));
    }

    #[test]
    fn test_server_variable_builder() {
        let var = ServerVariable::builder()
            .default("8080".to_string())
            .description("Port number".to_string())
            .build();

        assert_eq!(var.default, "8080");
        assert_eq!(var.description, Some("Port number".to_string()));
    }

    #[test]
    fn test_server_serialization() {
        let server = Server::new("test", "https://example.com").with_summary("Test server");

        let json_value = serde_json::to_value(&server).unwrap();
        let expected = json!({
            "name": "test",
            "url": "https://example.com",
            "summary": "Test server"
        });

        assert_eq!(json_value, expected);

        let deserialized: Server = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, server);
    }

    #[test]
    fn test_server_variable_serialization() {
        let var =
            ServerVariable::new("8080").with_enum(vec!["8080".to_string(), "8443".to_string()]);

        let json_value = serde_json::to_value(&var).unwrap();
        let expected = json!({
            "default": "8080",
            "enum": ["8080", "8443"]
        });

        assert_eq!(json_value, expected);

        let deserialized: ServerVariable = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, var);
    }

    #[test]
    fn test_server_with_extensions() {
        let server = Server::new("test", "https://example.com").with_extension("x-custom", "value");

        assert!(!server.extensions.is_empty());
        assert_eq!(server.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn test_server_variable_with_extensions() {
        let var = ServerVariable::new("test").with_extension("x-custom", "value");

        assert!(!var.extensions.is_empty());
        assert_eq!(var.extensions.get("x-custom"), Some(&json!("value")));
    }
}
