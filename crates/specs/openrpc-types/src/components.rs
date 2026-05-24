//! Components Object for OpenRPC specification.

use crate::{
    ContentDescriptor, ErrorObject, Example, ExamplePairing, Extensions, Link, Schema, Tag,
    error::OpenRpcResult, validation::Validate,
};
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Holds a set of reusable objects for different aspects of the OpenRPC.
/// All objects defined within the components object will have no effect on the API
/// unless they are explicitly referenced from properties outside the components object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Components {
    /// An object to hold reusable Content Descriptor Objects.
    #[serde(rename = "contentDescriptors", skip_serializing_if = "Option::is_none")]
    pub content_descriptors: Option<HashMap<String, ContentDescriptor>>,

    /// An object to hold reusable Schema Objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schemas: Option<HashMap<String, Schema>>,

    /// An object to hold reusable Example Objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<HashMap<String, Example>>,

    /// An object to hold reusable Link Objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<HashMap<String, Link>>,

    /// An object to hold reusable Error Objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<HashMap<String, ErrorObject>>,

    /// An object to hold reusable Example Pairing Objects.
    #[serde(
        rename = "examplePairingObjects",
        skip_serializing_if = "Option::is_none"
    )]
    pub example_pairings: Option<HashMap<String, ExamplePairing>>,

    /// An object to hold reusable Tag Objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, Tag>>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Components {
    /// Create a new empty Components object
    pub fn new() -> Self {
        Self {
            content_descriptors: None,
            schemas: None,
            examples: None,
            links: None,
            errors: None,
            example_pairings: None,
            tags: None,
            extensions: Extensions::new(),
        }
    }

    /// Set content descriptors
    pub fn with_content_descriptors(
        mut self,
        content_descriptors: HashMap<String, ContentDescriptor>,
    ) -> Self {
        self.content_descriptors = Some(content_descriptors);
        self
    }

    /// Add a content descriptor
    pub fn with_content_descriptor(
        mut self,
        name: impl Into<String>,
        content_descriptor: ContentDescriptor,
    ) -> Self {
        if self.content_descriptors.is_none() {
            self.content_descriptors = Some(HashMap::new());
        }
        self.content_descriptors
            .as_mut()
            .unwrap()
            .insert(name.into(), content_descriptor);
        self
    }

    /// Set schemas
    pub fn with_schemas(mut self, schemas: HashMap<String, Schema>) -> Self {
        self.schemas = Some(schemas);
        self
    }

    /// Add a schema
    pub fn with_schema(mut self, name: impl Into<String>, schema: Schema) -> Self {
        if self.schemas.is_none() {
            self.schemas = Some(HashMap::new());
        }
        self.schemas.as_mut().unwrap().insert(name.into(), schema);
        self
    }

    /// Set examples
    pub fn with_examples(mut self, examples: HashMap<String, Example>) -> Self {
        self.examples = Some(examples);
        self
    }

    /// Add an example
    pub fn with_example(mut self, name: impl Into<String>, example: Example) -> Self {
        if self.examples.is_none() {
            self.examples = Some(HashMap::new());
        }
        self.examples.as_mut().unwrap().insert(name.into(), example);
        self
    }

    /// Set links
    pub fn with_links(mut self, links: HashMap<String, Link>) -> Self {
        self.links = Some(links);
        self
    }

    /// Add a link
    pub fn with_link(mut self, name: impl Into<String>, link: Link) -> Self {
        if self.links.is_none() {
            self.links = Some(HashMap::new());
        }
        self.links.as_mut().unwrap().insert(name.into(), link);
        self
    }

    /// Set errors
    pub fn with_errors(mut self, errors: HashMap<String, ErrorObject>) -> Self {
        self.errors = Some(errors);
        self
    }

    /// Add an error
    pub fn with_error(mut self, name: impl Into<String>, error: ErrorObject) -> Self {
        if self.errors.is_none() {
            self.errors = Some(HashMap::new());
        }
        self.errors.as_mut().unwrap().insert(name.into(), error);
        self
    }

    /// Set example pairings
    pub fn with_example_pairings(
        mut self,
        example_pairings: HashMap<String, ExamplePairing>,
    ) -> Self {
        self.example_pairings = Some(example_pairings);
        self
    }

    /// Add an example pairing
    pub fn with_example_pairing(
        mut self,
        name: impl Into<String>,
        example_pairing: ExamplePairing,
    ) -> Self {
        if self.example_pairings.is_none() {
            self.example_pairings = Some(HashMap::new());
        }
        self.example_pairings
            .as_mut()
            .unwrap()
            .insert(name.into(), example_pairing);
        self
    }

    /// Set tags
    pub fn with_tags(mut self, tags: HashMap<String, Tag>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, name: impl Into<String>, tag: Tag) -> Self {
        if self.tags.is_none() {
            self.tags = Some(HashMap::new());
        }
        self.tags.as_mut().unwrap().insert(name.into(), tag);
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

    /// Check if components is empty
    pub fn is_empty(&self) -> bool {
        self.content_descriptors
            .as_ref()
            .is_none_or(|m| m.is_empty())
            && self.schemas.as_ref().is_none_or(|m| m.is_empty())
            && self.examples.as_ref().is_none_or(|m| m.is_empty())
            && self.links.as_ref().is_none_or(|m| m.is_empty())
            && self.errors.as_ref().is_none_or(|m| m.is_empty())
            && self.example_pairings.as_ref().is_none_or(|m| m.is_empty())
            && self.tags.as_ref().is_none_or(|m| m.is_empty())
            && self.extensions.is_empty()
    }

    /// Get a content descriptor by name
    pub fn get_content_descriptor(&self, name: &str) -> Option<&ContentDescriptor> {
        self.content_descriptors.as_ref()?.get(name)
    }

    /// Get a schema by name
    pub fn get_schema(&self, name: &str) -> Option<&Schema> {
        self.schemas.as_ref()?.get(name)
    }

    /// Get an example by name
    pub fn get_example(&self, name: &str) -> Option<&Example> {
        self.examples.as_ref()?.get(name)
    }

    /// Get a link by name
    pub fn get_link(&self, name: &str) -> Option<&Link> {
        self.links.as_ref()?.get(name)
    }

    /// Get an error by name
    pub fn get_error(&self, name: &str) -> Option<&ErrorObject> {
        self.errors.as_ref()?.get(name)
    }

    /// Get an example pairing by name
    pub fn get_example_pairing(&self, name: &str) -> Option<&ExamplePairing> {
        self.example_pairings.as_ref()?.get(name)
    }

    /// Get a tag by name
    pub fn get_tag(&self, name: &str) -> Option<&Tag> {
        self.tags.as_ref()?.get(name)
    }
}

impl Default for Components {
    fn default() -> Self {
        Self::new()
    }
}

impl Validate for Components {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate component keys and values
        if let Some(ref content_descriptors) = self.content_descriptors {
            for (key, value) in content_descriptors {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("contentDescriptors.{}", key),
                    )
                })?;
            }
        }

        if let Some(ref schemas) = self.schemas {
            for (key, value) in schemas {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("schemas.{}", key),
                    )
                })?;
            }
        }

        if let Some(ref examples) = self.examples {
            for (key, value) in examples {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("examples.{}", key),
                    )
                })?;
            }
        }

        if let Some(ref links) = self.links {
            for (key, value) in links {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("links.{}", key),
                    )
                })?;
            }
        }

        if let Some(ref errors) = self.errors {
            for (key, value) in errors {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("errors.{}", key),
                    )
                })?;
            }
        }

        if let Some(ref example_pairings) = self.example_pairings {
            for (key, value) in example_pairings {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("examplePairingObjects.{}", key),
                    )
                })?;
            }
        }

        if let Some(ref tags) = self.tags {
            for (key, value) in tags {
                crate::validation::validate_component_key(key)?;
                value.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("tags.{}", key),
                    )
                })?;
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
    fn test_components_creation() {
        let components = Components::new()
            .with_schema(
                "UserSchema",
                Schema::object().with_property("name", Schema::string()),
            )
            .with_content_descriptor(
                "UserParam",
                ContentDescriptor::new("user", Schema::string()),
            )
            .with_example("UserExample", Example::with_value(json!({"name": "John"})));

        assert!(components.schemas.is_some());
        assert!(components.content_descriptors.is_some());
        assert!(components.examples.is_some());
        assert!(!components.is_empty());
    }

    #[test]
    fn test_components_getters() {
        let schema = Schema::string();
        let components = Components::new().with_schema("TestSchema", schema.clone());

        let retrieved = components.get_schema("TestSchema");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), &schema);

        let missing = components.get_schema("Missing");
        assert!(missing.is_none());
    }

    #[test]
    fn test_components_validation() {
        // Valid components
        let components = Components::new().with_schema("ValidSchema", Schema::string());
        assert!(components.validate().is_ok());

        // Invalid - bad component key
        let mut components = Components::new();
        let mut schemas = HashMap::new();
        schemas.insert("invalid key".to_string(), Schema::string());
        components.schemas = Some(schemas);
        assert!(components.validate().is_err());
    }

    #[test]
    fn test_components_is_empty() {
        let empty_components = Components::new();
        assert!(empty_components.is_empty());

        let non_empty = Components::new().with_schema("Test", Schema::string());
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_components_builder() {
        let schema = Schema::string();
        let components = Components::builder()
            .schemas(HashMap::from([("TestSchema".to_string(), schema)]))
            .build();

        assert!(components.schemas.is_some());
        assert_eq!(components.schemas.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_components_serialization() {
        let components =
            Components::new().with_schema("UserSchema", Schema::string().with_title("User Name"));

        let json_value = serde_json::to_value(&components).unwrap();

        // Check structure
        assert!(json_value["schemas"].is_object());
        assert!(json_value["schemas"]["UserSchema"].is_object());
        assert_eq!(json_value["schemas"]["UserSchema"]["type"], "string");
        assert_eq!(json_value["schemas"]["UserSchema"]["title"], "User Name");

        let deserialized: Components = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, components);
    }

    #[test]
    fn test_components_with_all_types() {
        let components = Components::new()
            .with_schema("Schema1", Schema::string())
            .with_content_descriptor("Param1", ContentDescriptor::new("param", Schema::number()))
            .with_example("Example1", Example::with_value("test"))
            .with_link("Link1", Link::new("testLink"))
            .with_error("Error1", ErrorObject::new(1000, "Test error"))
            .with_example_pairing("Pairing1", ExamplePairing::new("test", vec![]))
            .with_tag("Tag1", Tag::new("test"));

        assert!(components.schemas.is_some());
        assert!(components.content_descriptors.is_some());
        assert!(components.examples.is_some());
        assert!(components.links.is_some());
        assert!(components.errors.is_some());
        assert!(components.example_pairings.is_some());
        assert!(components.tags.is_some());
        assert!(!components.is_empty());

        // Test validation
        assert!(components.validate().is_ok());
    }

    #[test]
    fn test_components_with_extensions() {
        let components = Components::new().with_extension("x-custom", "value");

        assert!(!components.extensions.is_empty());
        assert_eq!(components.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn test_components_all_getters() {
        let components = Components::new()
            .with_schema("Schema1", Schema::string())
            .with_content_descriptor("Param1", ContentDescriptor::new("param", Schema::string()))
            .with_example("Example1", Example::with_value("test"))
            .with_link("Link1", Link::new("testLink"))
            .with_error("Error1", ErrorObject::new(1000, "Test error"))
            .with_example_pairing("Pairing1", ExamplePairing::new("test", vec![]))
            .with_tag("Tag1", Tag::new("test"));

        assert!(components.get_schema("Schema1").is_some());
        assert!(components.get_content_descriptor("Param1").is_some());
        assert!(components.get_example("Example1").is_some());
        assert!(components.get_link("Link1").is_some());
        assert!(components.get_error("Error1").is_some());
        assert!(components.get_example_pairing("Pairing1").is_some());
        assert!(components.get_tag("Tag1").is_some());

        // Test missing items
        assert!(components.get_schema("Missing").is_none());
        assert!(components.get_content_descriptor("Missing").is_none());
    }

    #[test]
    fn map_setters_replace_each_component_collection() {
        let components = Components::new()
            .with_content_descriptors(HashMap::from([(
                "UserParam".to_string(),
                ContentDescriptor::new("user", Schema::string()),
            )]))
            .with_schemas(HashMap::from([("User".to_string(), Schema::object())]))
            .with_examples(HashMap::from([(
                "UserExample".to_string(),
                Example::with_value(json!({"id": "user-1"})),
            )]))
            .with_links(HashMap::from([(
                "ProfileLink".to_string(),
                Link::new("profile").with_method("getProfile"),
            )]))
            .with_errors(HashMap::from([(
                "UserNotFound".to_string(),
                ErrorObject::new(1000, "User not found"),
            )]))
            .with_example_pairings(HashMap::from([(
                "UserPairing".to_string(),
                ExamplePairing::new("userPairing", vec![]),
            )]))
            .with_tags(HashMap::from([("Users".to_string(), Tag::new("users"))]));

        assert!(components.validate().is_ok());
        assert!(components.get_content_descriptor("UserParam").is_some());
        assert!(components.get_schema("User").is_some());
        assert!(components.get_example("UserExample").is_some());
        assert!(components.get_link("ProfileLink").is_some());
        assert!(components.get_error("UserNotFound").is_some());
        assert!(components.get_example_pairing("UserPairing").is_some());
        assert!(components.get_tag("Users").is_some());
    }

    #[test]
    fn validation_errors_include_component_collection_paths() {
        let cases = [
            (
                Components::new().with_content_descriptor(
                    "BadParam",
                    ContentDescriptor::new("bad name", Schema::string()),
                ),
                "Validation error at contentDescriptors.BadParam",
            ),
            (
                Components::new().with_schema(
                    "BadSchema",
                    Schema::string().with_min_length(10).with_max_length(1),
                ),
                "Validation error at schemas.BadSchema",
            ),
            (
                Components::new().with_example("BadExample", {
                    let mut example = Example::with_value("inline");
                    example.external_value = Some("https://example.test/value.json".to_string());
                    example
                }),
                "Validation error at examples.BadExample",
            ),
            (
                Components::new()
                    .with_link("BadLink", Link::new("badLink").with_method("rpc.private")),
                "Validation error at links.BadLink",
            ),
            (
                Components::new().with_error("BadError", ErrorObject::new(1000, "")),
                "Validation error at errors.BadError",
            ),
            (
                Components::new()
                    .with_example_pairing("BadPairing", ExamplePairing::new("", vec![])),
                "Validation error at examplePairingObjects.BadPairing",
            ),
            (
                Components::new().with_tag("BadTag", Tag::new("")),
                "Validation error at tags.BadTag",
            ),
        ];

        for (components, path) in cases {
            let error = components.validate().unwrap_err().to_string();
            assert!(
                error.starts_with(path),
                "expected `{error}` to start with `{path}`"
            );
        }
    }

    #[test]
    fn component_key_validation_runs_before_nested_value_validation() {
        let components = Components::new().with_schema(
            "invalid key",
            Schema::string().with_min_length(10).with_max_length(1),
        );

        assert_eq!(
            components.validate().unwrap_err().to_string(),
            "Validation error: Invalid component key character ' ' in key 'invalid key'"
        );
    }
}
