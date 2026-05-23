//! Example Object and Example Pairing Object for OpenRPC specification.

use crate::{Extensions, Reference, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The Example object is an object that defines an example that is intended
/// to match the schema of a given Content Descriptor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Example {
    /// Canonical name of the example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Short description for the example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A verbose explanation of the example.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Embedded literal example.
    /// The value field and externalValue field are mutually exclusive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,

    /// A URL that points to the literal example.
    /// This provides the capability to reference examples that cannot easily be included in JSON documents.
    /// The value field and externalValue field are mutually exclusive.
    #[serde(rename = "externalValue", skip_serializing_if = "Option::is_none")]
    pub external_value: Option<String>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Example {
    /// Create a new Example with embedded value
    pub fn with_value(value: impl Into<Value>) -> Self {
        Self {
            name: None,
            summary: None,
            description: None,
            value: Some(value.into()),
            external_value: None,
            extensions: Extensions::new(),
        }
    }

    /// Create a new Example with external value URL
    pub fn with_external_value(external_value: impl Into<String>) -> Self {
        Self {
            name: None,
            summary: None,
            description: None,
            value: None,
            external_value: Some(external_value.into()),
            extensions: Extensions::new(),
        }
    }

    /// Create a new empty Example
    pub fn new() -> Self {
        Self {
            name: None,
            summary: None,
            description: None,
            value: None,
            external_value: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
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

    /// Set the value (clears external_value)
    pub fn set_value(mut self, value: impl Into<Value>) -> Self {
        self.value = Some(value.into());
        self.external_value = None;
        self
    }

    /// Set the external value URL (clears value)
    pub fn set_external_value(mut self, external_value: impl Into<String>) -> Self {
        self.external_value = Some(external_value.into());
        self.value = None;
        self
    }

    /// Add an extension field
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions
            .insert(key, value)
            .expect("extension keys must start with 'x-'");
        self
    }
}

impl Default for Example {
    fn default() -> Self {
        Self::new()
    }
}

impl Validate for Example {
    fn validate(&self) -> OpenRpcResult<()> {
        // Value and externalValue are mutually exclusive
        if self.value.is_some() && self.external_value.is_some() {
            return Err(crate::error::OpenRpcError::validation(
                "value and externalValue fields are mutually exclusive",
            ));
        }

        // Validate external value URL if present
        if let Some(ref external_value) = self.external_value {
            crate::validation::validate_url(external_value)?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

/// The Example Pairing object consists of a set of example params and result.
/// The result is what you can expect from the JSON-RPC service given the exact params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct ExamplePairing {
    /// Name for the example pairing.
    pub name: String,

    /// A verbose explanation of the example pairing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Short description for the example pairing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Example parameters.
    pub params: Vec<ExampleOrReference>,

    /// Example result.
    /// When undefined, the example pairing represents usage of the method as a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ExampleOrReference>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

/// Example or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExampleOrReference {
    Example(Box<Example>),
    Reference(Reference),
}

impl ExamplePairing {
    /// Create a new ExamplePairing with required fields
    pub fn new(name: impl Into<String>, params: Vec<ExampleOrReference>) -> Self {
        Self {
            name: name.into(),
            description: None,
            summary: None,
            params,
            result: None,
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

    /// Set the result
    pub fn with_result(mut self, result: ExampleOrReference) -> Self {
        self.result = Some(result);
        self
    }

    /// Add an extension field
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions
            .insert(key, value)
            .expect("extension keys must start with 'x-'");
        self
    }

    /// Check if this represents a notification (no result)
    pub fn is_notification(&self) -> bool {
        self.result.is_none()
    }
}

impl Validate for ExamplePairing {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }

        // Validate params
        for (i, param) in self.params.iter().enumerate() {
            param.validate().map_err(|e| {
                crate::error::OpenRpcError::validation_with_path(
                    e.to_string(),
                    format!("params[{}]", i),
                )
            })?;
        }

        // Validate result if present
        if let Some(ref result) = self.result {
            result.validate().map_err(|e| {
                crate::error::OpenRpcError::validation_with_path(
                    e.to_string(),
                    "result".to_string(),
                )
            })?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

impl Validate for ExampleOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            ExampleOrReference::Example(example) => example.as_ref().validate(),
            ExampleOrReference::Reference(reference) => reference.validate(),
        }
    }
}

// Convenience constructors
impl From<Example> for ExampleOrReference {
    fn from(example: Example) -> Self {
        ExampleOrReference::Example(Box::new(example))
    }
}

impl From<Reference> for ExampleOrReference {
    fn from(reference: Reference) -> Self {
        ExampleOrReference::Reference(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_example_creation() {
        let example = Example::with_value("test_value")
            .with_name("Test Example")
            .with_description("A test example");

        assert_eq!(example.name, Some("Test Example".to_string()));
        assert_eq!(example.value, Some(Value::String("test_value".to_string())));
        assert!(example.external_value.is_none());
    }

    #[test]
    fn test_example_external_value() {
        let example = Example::with_external_value("https://example.com/test.json")
            .with_summary("External example");

        assert_eq!(
            example.external_value,
            Some("https://example.com/test.json".to_string())
        );
        assert!(example.value.is_none());
    }

    #[test]
    fn test_example_validation() {
        // Valid example with value
        let example = Example::with_value("test");
        assert!(example.validate().is_ok());

        // Valid example with external value
        let example = Example::with_external_value("https://example.com");
        assert!(example.validate().is_ok());

        // Invalid - both value and external value
        let mut example = Example::with_value("test");
        example.external_value = Some("https://example.com".to_string());
        assert!(example.validate().is_err());

        // Invalid - bad external value URL
        let example = Example::with_external_value("not-a-url");
        assert!(example.validate().is_err());
    }

    #[test]
    fn test_example_pairing_creation() {
        let params = vec![
            ExampleOrReference::Example(Box::new(Example::with_value("param1"))),
            ExampleOrReference::Example(Box::new(Example::with_value(42))),
        ];
        let result = ExampleOrReference::Example(Box::new(Example::with_value("result")));

        let pairing = ExamplePairing::new("test_pairing", params)
            .with_description("A test pairing")
            .with_result(result);

        assert_eq!(pairing.name, "test_pairing");
        assert_eq!(pairing.params.len(), 2);
        assert!(pairing.result.is_some());
        assert!(!pairing.is_notification());
    }

    #[test]
    fn test_example_pairing_notification() {
        let params = vec![ExampleOrReference::Example(Box::new(Example::with_value(
            "param",
        )))];
        let pairing = ExamplePairing::new("notification", params);

        assert!(pairing.is_notification());
    }

    #[test]
    fn test_example_pairing_validation() {
        let params = vec![ExampleOrReference::Example(Box::new(Example::with_value(
            "test",
        )))];

        // Valid pairing
        let pairing = ExamplePairing::new("valid", params.clone());
        assert!(pairing.validate().is_ok());

        // Invalid - empty name
        let pairing = ExamplePairing::new("", params);
        assert!(pairing.validate().is_err());
    }

    #[test]
    fn test_example_builder() {
        let example = Example::builder()
            .name("test".to_string())
            .value(json!("test_value"))
            .build();

        assert_eq!(example.name, Some("test".to_string()));
        assert_eq!(example.value, Some(json!("test_value")));
    }

    #[test]
    fn test_example_pairing_builder() {
        let params = vec![ExampleOrReference::Example(Box::new(Example::with_value(
            "test",
        )))];
        let pairing = ExamplePairing::builder()
            .name("test_pairing".to_string())
            .params(params)
            .build();

        assert_eq!(pairing.name, "test_pairing");
        assert_eq!(pairing.params.len(), 1);
    }

    #[test]
    fn test_example_serialization() {
        let example = Example::with_value(json!({"key": "value"})).with_name("test");

        let json_value = serde_json::to_value(&example).unwrap();
        let expected = json!({
            "name": "test",
            "value": {"key": "value"}
        });

        assert_eq!(json_value, expected);

        let deserialized: Example = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, example);
    }

    #[test]
    fn test_example_or_reference() {
        let example_variant = ExampleOrReference::Example(Box::new(Example::with_value("test")));
        let ref_variant = ExampleOrReference::Reference(Reference::example("TestExample"));

        // Test serialization
        let example_json = serde_json::to_value(&example_variant).unwrap();
        assert!(example_json["value"] == "test");

        let ref_json = serde_json::to_value(&ref_variant).unwrap();
        assert!(ref_json["$ref"] == "#/components/examples/TestExample");
    }

    #[test]
    fn test_example_with_extensions() {
        let example = Example::with_value("test").with_extension("x-custom", "value");

        assert!(!example.extensions.is_empty());
        assert_eq!(example.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn example_setters_keep_value_and_external_value_mutually_exclusive() {
        let example = Example::default()
            .with_name("sample")
            .with_summary("short")
            .with_description("long")
            .set_external_value("https://example.com/value.json");

        assert_eq!(example.name.as_deref(), Some("sample"));
        assert_eq!(example.summary.as_deref(), Some("short"));
        assert_eq!(example.description.as_deref(), Some("long"));
        assert_eq!(
            example.external_value.as_deref(),
            Some("https://example.com/value.json")
        );
        assert!(example.value.is_none());

        let example = example.set_value(json!({"inline": true}));
        assert_eq!(example.value, Some(json!({"inline": true})));
        assert!(example.external_value.is_none());
        assert!(example.validate().is_ok());
    }

    #[test]
    fn example_validation_rejects_invalid_extension_maps() {
        let invalid_extensions: Extensions =
            HashMap::from([("x-".to_string(), json!("missing suffix"))]).into();
        let example = Example {
            extensions: invalid_extensions,
            ..Example::with_value("test")
        };

        let err = example.validate().unwrap_err();
        assert!(err.to_string().contains("Extension key must have content"));
    }

    #[test]
    fn example_pairing_helpers_set_summary_result_and_extensions() {
        let pairing = ExamplePairing::new("pairing", vec![Example::with_value("param").into()])
            .with_summary("short")
            .with_result(Example::with_value("result").into())
            .with_extension("x-scope", "docs");

        assert_eq!(pairing.summary.as_deref(), Some("short"));
        assert!(pairing.result.is_some());
        assert_eq!(pairing.extensions.get("x-scope"), Some(&json!("docs")));
        assert!(!pairing.is_notification());
        assert!(pairing.validate().is_ok());
    }

    #[test]
    fn example_pairing_validation_reports_nested_param_and_result_paths() {
        let pairing = ExamplePairing::new(
            "bad-param",
            vec![Example::with_external_value("not a url").into()],
        );
        let err = pairing.validate().unwrap_err();
        assert!(err.to_string().contains("params[0]"));

        let pairing = ExamplePairing::new("bad-result", vec![])
            .with_result(Example::with_external_value("not a url").into());
        let err = pairing.validate().unwrap_err();
        assert!(err.to_string().contains("result"));
    }

    #[test]
    fn example_or_reference_from_conversions_validate_both_variants() {
        let example: ExampleOrReference = Example::with_value(json!({"id": 1})).into();
        assert!(matches!(example, ExampleOrReference::Example(_)));
        assert!(example.validate().is_ok());

        let reference: ExampleOrReference = Reference::example("CreatedUser").into();
        assert!(matches!(reference, ExampleOrReference::Reference(_)));
        assert!(reference.validate().is_ok());

        let invalid_reference = ExampleOrReference::Reference(Reference::new(""));
        let err = invalid_reference.validate().unwrap_err();
        assert!(err.to_string().contains("Reference string cannot be empty"));
    }
}
