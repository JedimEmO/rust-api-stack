//! Link Object for OpenRPC specification.

use crate::{Extensions, Server, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// The Link object represents a possible design-time link for a result.
/// The presence of a link does not guarantee the caller's ability to successfully invoke it,
/// rather it provides a known relationship and traversal mechanism between results and other methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// Canonical name of the link.
    pub name: String,

    /// A description of the link.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Short description for the link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// The name of an existing, resolvable OpenRPC method, as defined with a unique method.
    /// This field MUST resolve to a unique Method Object. As opposed to Open Api,
    /// Relative method values ARE NOT permitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    /// A map representing parameters to pass to a method as specified with method.
    /// The key is the parameter name to be used, whereas the value can be a constant
    /// or a runtime expression to be evaluated and passed to the linked method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,

    /// A server object to be used by the target method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Link {
    /// Create a new Link with required name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            summary: None,
            method: None,
            params: None,
            server: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the summary
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Set the method name
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Set the parameters
    pub fn with_params(mut self, params: HashMap<String, Value>) -> Self {
        self.params = Some(params);
        self
    }

    /// Add a parameter
    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        if self.params.is_none() {
            self.params = Some(HashMap::new());
        }
        self.params
            .as_mut()
            .unwrap()
            .insert(name.into(), value.into());
        self
    }

    /// Set the server
    pub fn with_server(mut self, server: Server) -> Self {
        self.server = Some(server);
        self
    }

    /// Add an extension field
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

impl Validate for Link {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }

        // Validate method name if present
        if let Some(ref method) = self.method {
            crate::validation::validate_method_name(method)?;
        }

        // Validate server if present
        if let Some(ref server) = self.server {
            server.validate()?;
        }

        // Validate parameters - keys should be valid parameter names
        if let Some(ref params) = self.params {
            for key in params.keys() {
                if key.is_empty() {
                    return Err(crate::error::OpenRpcError::validation(
                        "parameter name cannot be empty",
                    ));
                }
            }
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

/// Runtime Expression for Link parameters
///
/// Runtime expressions allow the user to define an expression which will evaluate
/// to a string once the desired value(s) are known. They are used when the desired
/// value of a link or server can only be constructed at run time.
///
/// The runtime expression makes use of JSON Template Language syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeExpression(pub String);

impl RuntimeExpression {
    /// Create a new runtime expression
    pub fn new(expression: impl Into<String>) -> Self {
        Self(expression.into())
    }

    /// Create a runtime expression that references a result value
    pub fn result(path: &str) -> Self {
        Self(format!("$result.{}", path))
    }

    /// Create a runtime expression that references a parameter value
    pub fn param(name: &str) -> Self {
        Self(format!("$params.{}", name))
    }

    /// Create a runtime expression that references the method name
    pub fn method() -> Self {
        Self("$method".to_string())
    }

    /// Get the expression string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RuntimeExpression {
    fn from(expression: String) -> Self {
        Self(expression)
    }
}

impl From<&str> for RuntimeExpression {
    fn from(expression: &str) -> Self {
        Self(expression.to_string())
    }
}

impl std::fmt::Display for RuntimeExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_link_creation() {
        let link = Link::new("getUserProfile")
            .with_description("Get user profile information")
            .with_method("getUser")
            .with_param("userId", "$result.id");

        assert_eq!(link.name, "getUserProfile");
        assert_eq!(link.method, Some("getUser".to_string()));
        assert!(link.params.is_some());
        assert_eq!(
            link.params.as_ref().unwrap().get("userId"),
            Some(&json!("$result.id"))
        );
    }

    #[test]
    fn test_link_with_server() {
        let server = Server::new("api", "https://api.example.com");
        let link = Link::new("getRelated").with_server(server.clone());

        assert_eq!(link.server, Some(server));
    }

    #[test]
    fn test_link_validation() {
        // Valid link
        let link = Link::new("valid_link").with_method("validMethod");
        assert!(link.validate().is_ok());

        // Invalid - empty name
        let link = Link::new("");
        assert!(link.validate().is_err());

        // Invalid - invalid method name
        let link = Link::new("test").with_method("rpc.custom");
        assert!(link.validate().is_err());
    }

    #[test]
    fn test_runtime_expression() {
        let expr = RuntimeExpression::result("user.id");
        assert_eq!(expr.as_str(), "$result.user.id");

        let expr = RuntimeExpression::param("userId");
        assert_eq!(expr.as_str(), "$params.userId");

        let expr = RuntimeExpression::method();
        assert_eq!(expr.as_str(), "$method");

        let expr = RuntimeExpression::new("custom.expression");
        assert_eq!(expr.as_str(), "custom.expression");
    }

    #[test]
    fn test_runtime_expression_conversion() {
        let expr: RuntimeExpression = "test.expression".into();
        assert_eq!(expr.as_str(), "test.expression");

        let expr: RuntimeExpression = String::from("test.expression").into();
        assert_eq!(expr.as_str(), "test.expression");
    }

    #[test]
    fn test_link_builder() {
        let link = Link::builder()
            .name("testLink".to_string())
            .summary("Test link".to_string())
            .method("testMethod".to_string())
            .build();

        assert_eq!(link.name, "testLink");
        assert_eq!(link.summary, Some("Test link".to_string()));
        assert_eq!(link.method, Some("testMethod".to_string()));
    }

    #[test]
    fn test_link_serialization() {
        let mut params = HashMap::new();
        params.insert("id".to_string(), json!("$result.userId"));

        let link = Link::new("getUserData")
            .with_method("getUser")
            .with_params(params);

        let json_value = serde_json::to_value(&link).unwrap();
        let expected = json!({
            "name": "getUserData",
            "method": "getUser",
            "params": {
                "id": "$result.userId"
            }
        });

        assert_eq!(json_value, expected);

        let deserialized: Link = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, link);
    }

    #[test]
    fn test_link_with_extensions() {
        let link = Link::new("test").with_extension("x-custom", "value");

        assert!(!link.extensions.is_empty());
        assert_eq!(link.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn test_runtime_expression_serialization() {
        let expr = RuntimeExpression::result("user.id");

        let json_value = serde_json::to_value(&expr).unwrap();
        assert_eq!(json_value, json!("$result.user.id"));

        let deserialized: RuntimeExpression = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized.as_str(), "$result.user.id");
    }

    #[test]
    fn test_runtime_expression_display() {
        let expr = RuntimeExpression::result("test.value");
        assert_eq!(format!("{}", expr), "$result.test.value");
    }
}
