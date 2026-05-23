//! Content Descriptor Object for OpenRPC specification.

use crate::{Extensions, Reference, Schema, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// Content Descriptors are objects that describe content.
/// They are reusable ways of describing either parameters or result.
/// They MUST have a schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct ContentDescriptor {
    /// Name of the content that is being described.
    /// If the content described is a method parameter assignable by-name,
    /// this field SHALL define the parameter's key (ie name).
    pub name: String,

    /// A short summary of the content that is being described.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A verbose explanation of the content descriptor behavior.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Determines if the content is a required field.
    /// Default value is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Schema that describes the content.
    pub schema: ContentDescriptorSchema,

    /// Specifies that the content is deprecated and SHOULD be transitioned out of usage.
    /// Default value is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

/// Schema or Reference Object for content descriptor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentDescriptorSchema {
    Schema(Box<Schema>),
    Reference(Reference),
}

impl ContentDescriptor {
    /// Create a new ContentDescriptor with required fields
    pub fn new(name: impl Into<String>, schema: Schema) -> Self {
        Self {
            name: name.into(),
            summary: None,
            description: None,
            required: None,
            schema: ContentDescriptorSchema::Schema(Box::new(schema)),
            deprecated: None,
            extensions: Extensions::new(),
        }
    }

    /// Create a new ContentDescriptor with a schema reference
    pub fn with_reference(name: impl Into<String>, reference: Reference) -> Self {
        Self {
            name: name.into(),
            summary: None,
            description: None,
            required: None,
            schema: ContentDescriptorSchema::Reference(reference),
            deprecated: None,
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

    /// Set whether this content is required
    pub fn with_required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }

    /// Mark this content as required
    pub fn required(mut self) -> Self {
        self.required = Some(true);
        self
    }

    /// Mark this content as optional
    pub fn optional(mut self) -> Self {
        self.required = Some(false);
        self
    }

    /// Set whether this content is deprecated
    pub fn with_deprecated(mut self, deprecated: bool) -> Self {
        self.deprecated = Some(deprecated);
        self
    }

    /// Mark this content as deprecated
    pub fn deprecated(mut self) -> Self {
        self.deprecated = Some(true);
        self
    }

    /// Add an extension field
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions
            .insert(key, value)
            .expect("extension keys must start with 'x-'");
        self
    }

    /// Check if this content descriptor is required (defaults to false)
    pub fn is_required(&self) -> bool {
        self.required.unwrap_or(false)
    }

    /// Check if this content descriptor is deprecated (defaults to false)
    pub fn is_deprecated(&self) -> bool {
        self.deprecated.unwrap_or(false)
    }
}

impl Validate for ContentDescriptor {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }
        crate::validation::validate_content_descriptor_name(&self.name)?;

        // Validate schema
        match &self.schema {
            ContentDescriptorSchema::Schema(schema) => schema.as_ref().validate()?,
            ContentDescriptorSchema::Reference(reference) => reference.validate()?,
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

impl Validate for ContentDescriptorSchema {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            ContentDescriptorSchema::Schema(schema) => schema.as_ref().validate(),
            ContentDescriptorSchema::Reference(reference) => reference.validate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_content_descriptor_creation() {
        let schema = Schema::string().with_min_length(1);
        let descriptor = ContentDescriptor::new("username", schema)
            .with_description("The user's username")
            .required();

        assert_eq!(descriptor.name, "username");
        assert_eq!(
            descriptor.description,
            Some("The user's username".to_string())
        );
        assert!(descriptor.is_required());
    }

    #[test]
    fn test_content_descriptor_with_reference() {
        let reference = Reference::schema("UserSchema");
        let descriptor =
            ContentDescriptor::with_reference("user", reference).with_summary("User object");

        assert_eq!(descriptor.name, "user");
        assert_eq!(descriptor.summary, Some("User object".to_string()));

        match descriptor.schema {
            ContentDescriptorSchema::Reference(ref r) => {
                assert_eq!(r.reference, "#/components/schemas/UserSchema");
            }
            _ => panic!("Expected reference"),
        }
    }

    #[test]
    fn test_content_descriptor_validation() {
        let schema = Schema::string();

        // Valid descriptor
        let descriptor = ContentDescriptor::new("valid_name", schema.clone());
        assert!(descriptor.validate().is_ok());

        // Invalid - empty name
        let descriptor = ContentDescriptor::new("", schema);
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn test_content_descriptor_required_optional() {
        let schema = Schema::string();

        let descriptor = ContentDescriptor::new("test", schema.clone());
        assert!(!descriptor.is_required()); // Default is false

        let descriptor = ContentDescriptor::new("test", schema.clone()).required();
        assert!(descriptor.is_required());

        let descriptor = ContentDescriptor::new("test", schema).optional();
        assert!(!descriptor.is_required());
    }

    #[test]
    fn test_content_descriptor_deprecated() {
        let schema = Schema::string();

        let descriptor = ContentDescriptor::new("test", schema.clone());
        assert!(!descriptor.is_deprecated()); // Default is false

        let descriptor = ContentDescriptor::new("test", schema).deprecated();
        assert!(descriptor.is_deprecated());
    }

    #[test]
    fn test_content_descriptor_builder() {
        let schema = Schema::string();
        let descriptor = ContentDescriptor::builder()
            .name("test".to_string())
            .schema(ContentDescriptorSchema::Schema(Box::new(schema)))
            .required(true)
            .build();

        assert_eq!(descriptor.name, "test");
        assert!(descriptor.is_required());
    }

    #[test]
    fn test_content_descriptor_serialization() {
        let schema = Schema::string().with_min_length(1);
        let descriptor = ContentDescriptor::new("username", schema)
            .with_description("Username field")
            .required();

        let json = serde_json::to_value(&descriptor).unwrap();

        // Check that required fields are present
        assert_eq!(json["name"], "username");
        assert_eq!(json["description"], "Username field");
        assert_eq!(json["required"].as_bool(), Some(true));
        assert!(json["schema"].is_object());

        let deserialized: ContentDescriptor = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, descriptor);
    }

    #[test]
    fn test_content_descriptor_schema_serialization() {
        // Test schema variant
        let schema_variant = ContentDescriptorSchema::Schema(Box::new(Schema::string()));
        let json = serde_json::to_value(&schema_variant).unwrap();
        assert!(json["type"] == "string");

        // Test reference variant
        let ref_variant = ContentDescriptorSchema::Reference(Reference::schema("Test"));
        let json = serde_json::to_value(&ref_variant).unwrap();
        assert!(json["$ref"] == "#/components/schemas/Test");
    }

    #[test]
    fn test_content_descriptor_with_extensions() {
        let schema = Schema::string();
        let descriptor = ContentDescriptor::new("test", schema).with_extension("x-custom", "value");

        assert!(!descriptor.extensions.is_empty());
        assert_eq!(descriptor.extensions.get("x-custom"), Some(&json!("value")));
    }
}
