//! External Documentation Object for OpenRPC specification.

use crate::{Extensions, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// Allows referencing an external resource for extended documentation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct ExternalDocumentation {
    /// A verbose explanation of the target documentation.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The URL for the target documentation.
    /// Value MUST be in the format of a URL.
    pub url: String,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl ExternalDocumentation {
    /// Create a new ExternalDocumentation with required URL
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            description: None,
            url: url.into(),
            extensions: Extensions::new(),
        }
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

impl Validate for ExternalDocumentation {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate URL
        if self.url.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("url"));
        }
        crate::validation::validate_url(&self.url)?;

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
    fn test_external_docs_creation() {
        let docs = ExternalDocumentation::new("https://example.com/docs")
            .with_description("API Documentation");

        assert_eq!(docs.url, "https://example.com/docs");
        assert_eq!(docs.description, Some("API Documentation".to_string()));
    }

    #[test]
    fn test_external_docs_validation() {
        // Valid external docs
        let docs = ExternalDocumentation::new("https://example.com");
        assert!(docs.validate().is_ok());

        // Invalid - empty URL
        let docs = ExternalDocumentation::new("");
        assert!(docs.validate().is_err());

        // Invalid - bad URL format
        let docs = ExternalDocumentation::new("not-a-url");
        assert!(docs.validate().is_err());
    }

    #[test]
    fn test_external_docs_builder() {
        let docs = ExternalDocumentation::builder()
            .url("https://example.com".to_string())
            .description("Test docs".to_string())
            .build();

        assert_eq!(docs.url, "https://example.com");
        assert_eq!(docs.description, Some("Test docs".to_string()));
    }

    #[test]
    fn test_external_docs_serialization() {
        let docs = ExternalDocumentation::new("https://example.com").with_description("API docs");

        let json = serde_json::to_value(&docs).unwrap();
        let expected = json!({
            "url": "https://example.com",
            "description": "API docs"
        });

        assert_eq!(json, expected);

        let deserialized: ExternalDocumentation = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, docs);
    }

    #[test]
    fn test_external_docs_with_extensions() {
        let docs =
            ExternalDocumentation::new("https://example.com").with_extension("x-custom", "value");

        assert!(!docs.extensions.is_empty());
        assert_eq!(docs.extensions.get("x-custom"), Some(&json!("value")));
    }
}
