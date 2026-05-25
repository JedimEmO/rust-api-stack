//! Reference Object for OpenRPC specification.
//!
//! A simple object to allow referencing other components in the specification,
//! internally and externally.

use crate::error::OpenRpcResult;
use crate::validation::Validate;
use bon::Builder;
use serde::{Deserialize, Serialize};

/// A simple object to allow referencing other components in the specification,
/// internally and externally.
///
/// The Reference Object is defined by JSON Schema and follows the same structure,
/// behavior and rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// The reference string.
    #[serde(rename = "$ref")]
    pub reference: String,
}

impl Reference {
    /// Create a new reference with a custom reference string
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
        }
    }
    /// Create a new reference to a component
    pub fn component(component_type: &str, name: &str) -> Self {
        Self {
            reference: format!("#/components/{}/{}", component_type, name),
        }
    }

    /// Create a new reference to a schema component
    pub fn schema(name: &str) -> Self {
        Self::component("schemas", name)
    }

    /// Create a new reference to a content descriptor component
    pub fn content_descriptor(name: &str) -> Self {
        Self::component("contentDescriptors", name)
    }

    /// Create a new reference to an example component
    pub fn example(name: &str) -> Self {
        Self::component("examples", name)
    }

    /// Create a new reference to a link component
    pub fn link(name: &str) -> Self {
        Self::component("links", name)
    }

    /// Create a new reference to an error component
    pub fn error(name: &str) -> Self {
        Self::component("errors", name)
    }

    /// Create a new reference to an example pairing component
    pub fn example_pairing(name: &str) -> Self {
        Self::component("examplePairingObjects", name)
    }

    /// Create a new reference to a tag component
    pub fn tag(name: &str) -> Self {
        Self::component("tags", name)
    }

    /// Create a new external reference
    pub fn external(url: &str) -> Self {
        Self {
            reference: url.to_string(),
        }
    }

    /// Check if this is an internal reference (starts with #)
    pub fn is_internal(&self) -> bool {
        self.reference.starts_with('#')
    }

    /// Check if this is an external reference
    pub fn is_external(&self) -> bool {
        !self.is_internal()
    }

    /// Extract the component type and name for internal references
    /// Returns (component_type, name) or None for external references
    pub fn component_parts(&self) -> Option<(&str, &str)> {
        if !self.is_internal() {
            return None;
        }

        // Parse #/components/{type}/{name}
        let path = self.reference.strip_prefix("#/components/")?;
        let parts: Vec<&str> = path.splitn(2, '/').collect();
        if parts.len() == 2 {
            Some((parts[0], parts[1]))
        } else {
            None
        }
    }
}

impl Validate for Reference {
    fn validate(&self) -> OpenRpcResult<()> {
        if self.reference.is_empty() {
            return Err(crate::error::OpenRpcError::validation(
                "Reference string cannot be empty",
            ));
        }

        // If it's an internal reference, validate the format
        if self.is_internal() {
            if let Some((component_type, name)) = self.component_parts() {
                // Validate component type
                match component_type {
                    "schemas"
                    | "contentDescriptors"
                    | "examples"
                    | "links"
                    | "errors"
                    | "examplePairingObjects"
                    | "tags" => {
                        // Valid component type
                    }
                    _ => {
                        return Err(crate::error::OpenRpcError::validation(format!(
                            "Invalid component type in reference: {}",
                            component_type
                        )));
                    }
                }

                // Validate component name
                crate::validation::validate_component_key(name)?;
            } else if !self.reference.starts_with("#/") {
                return Err(crate::error::OpenRpcError::validation(format!(
                    "Invalid internal reference format: {}",
                    self.reference
                )));
            }
        } else {
            // For external references, validate URL format
            crate::validation::validate_url(&self.reference)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_creation() {
        let ref_obj = Reference::schema("UserSchema");
        assert_eq!(ref_obj.reference, "#/components/schemas/UserSchema");

        let ref_obj = Reference::content_descriptor("UserParam");
        assert_eq!(
            ref_obj.reference,
            "#/components/contentDescriptors/UserParam"
        );

        let ref_obj = Reference::external("https://example.com/schema.json");
        assert_eq!(ref_obj.reference, "https://example.com/schema.json");
    }

    #[test]
    fn test_reference_type_detection() {
        let internal_ref = Reference::schema("Test");
        assert!(internal_ref.is_internal());
        assert!(!internal_ref.is_external());

        let external_ref = Reference::external("https://example.com");
        assert!(!external_ref.is_internal());
        assert!(external_ref.is_external());
    }

    #[test]
    fn test_component_parts() {
        let ref_obj = Reference::schema("UserSchema");
        let parts = ref_obj.component_parts();
        assert_eq!(parts, Some(("schemas", "UserSchema")));

        let ref_obj = Reference::external("https://example.com");
        let parts = ref_obj.component_parts();
        assert_eq!(parts, None);
    }

    #[test]
    fn test_reference_validation() {
        // Valid internal reference
        let ref_obj = Reference::schema("ValidName");
        assert!(ref_obj.validate().is_ok());

        // Valid external reference
        let ref_obj = Reference::external("https://example.com");
        assert!(ref_obj.validate().is_ok());

        // Invalid - empty reference
        let ref_obj = Reference {
            reference: String::new(),
        };
        assert!(ref_obj.validate().is_err());

        // Invalid component type
        let ref_obj = Reference {
            reference: "#/components/invalid/Test".to_string(),
        };
        assert!(ref_obj.validate().is_err());

        // Invalid component name
        let ref_obj = Reference {
            reference: "#/components/schemas/invalid name".to_string(),
        };
        assert!(ref_obj.validate().is_err());
    }

    #[test]
    fn test_reference_serialization() {
        let ref_obj = Reference::schema("TestSchema");
        let json = serde_json::to_string(&ref_obj).unwrap();
        assert_eq!(json, "{\"$ref\":\"#/components/schemas/TestSchema\"}");

        let deserialized: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ref_obj);
    }

    #[test]
    fn test_reference_builder() {
        let ref_obj = Reference::builder()
            .reference("#/components/schemas/Test".to_string())
            .build();

        assert_eq!(ref_obj.reference, "#/components/schemas/Test");
    }
}
