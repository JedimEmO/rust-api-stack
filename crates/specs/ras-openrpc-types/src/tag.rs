//! Tag Object for OpenRPC specification.

use crate::{Extensions, ExternalDocumentation, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// Adds metadata to a single tag that is used by the Method Object.
/// It is not mandatory to have a Tag Object per tag defined in the Method Object instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    /// The name of the tag.
    pub name: String,

    /// A short summary of the tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A verbose explanation for the tag.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional external documentation for this tag.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocumentation>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Tag {
    /// Create a new Tag with required name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            summary: None,
            description: None,
            external_docs: None,
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

    /// Set the external documentation
    pub fn with_external_docs(mut self, external_docs: ExternalDocumentation) -> Self {
        self.external_docs = Some(external_docs);
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

impl Validate for Tag {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }

        // Validate external docs if present
        if let Some(ref external_docs) = self.external_docs {
            external_docs.validate()?;
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
    fn test_tag_creation() {
        let tag = Tag::new("user")
            .with_summary("User operations")
            .with_description("All operations related to user management");

        assert_eq!(tag.name, "user");
        assert_eq!(tag.summary, Some("User operations".to_string()));
        assert_eq!(
            tag.description,
            Some("All operations related to user management".to_string())
        );
    }

    #[test]
    fn test_tag_validation() {
        // Valid tag
        let tag = Tag::new("valid");
        assert!(tag.validate().is_ok());

        // Invalid - empty name
        let tag = Tag::new("");
        assert!(tag.validate().is_err());
    }

    #[test]
    fn test_tag_with_external_docs() {
        let external_docs = ExternalDocumentation::new("https://example.com/user-docs");
        let tag = Tag::new("user").with_external_docs(external_docs);

        assert!(tag.external_docs.is_some());
        assert_eq!(
            tag.external_docs.as_ref().unwrap().url,
            "https://example.com/user-docs"
        );
    }

    #[test]
    fn test_tag_builder() {
        let tag = Tag::builder()
            .name("test".to_string())
            .summary("Test tag".to_string())
            .build();

        assert_eq!(tag.name, "test");
        assert_eq!(tag.summary, Some("Test tag".to_string()));
    }

    #[test]
    fn test_tag_serialization() {
        let tag = Tag::new("user").with_summary("User operations");

        let json = serde_json::to_value(&tag).unwrap();
        let expected = json!({
            "name": "user",
            "summary": "User operations"
        });

        assert_eq!(json, expected);

        let deserialized: Tag = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, tag);
    }

    #[test]
    fn test_tag_with_extensions() {
        let tag = Tag::new("test").with_extension("x-custom", "value");

        assert!(!tag.extensions.is_empty());
        assert_eq!(tag.extensions.get("x-custom"), Some(&json!("value")));
    }
}
