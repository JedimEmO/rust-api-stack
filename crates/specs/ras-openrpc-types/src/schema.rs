//! Schema Object for OpenRPC specification.
//!
//! The Schema Object allows the definition of input and output data types.
//! The Schema Objects MUST follow the specifications outline in the
//! JSON Schema Specification Draft 7.

use crate::{Extensions, Reference, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// The Schema Object allows the definition of input and output data types.
///
/// The Schema Objects MUST follow the specifications outline in the
/// JSON Schema Specification Draft 7. Alternatively, any time a Schema Object
/// can be used, a Reference Object can be used in its place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
pub struct Schema {
    // Core JSON Schema Draft 7 fields
    /// JSON Schema version
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Schema identifier
    #[serde(rename = "$id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Schema reference
    #[serde(rename = "$ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// Schema comment
    #[serde(rename = "$comment", skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// Title of the schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description of the schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,

    /// Examples of valid values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<Value>>,

    /// Read-only property
    #[serde(rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,

    /// Write-only property
    #[serde(rename = "writeOnly", skip_serializing_if = "Option::is_none")]
    pub write_only: Option<bool>,

    // Type-specific fields
    /// JSON Schema type
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<SchemaType>,

    /// Allowed values (enum)
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,

    /// Constant value
    #[serde(rename = "const", skip_serializing_if = "Option::is_none")]
    pub const_value: Option<Value>,

    // Numeric validation
    /// Multiple of validation for numbers
    #[serde(rename = "multipleOf", skip_serializing_if = "Option::is_none")]
    pub multiple_of: Option<f64>,

    /// Maximum value (inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,

    /// Exclusive maximum value
    #[serde(rename = "exclusiveMaximum", skip_serializing_if = "Option::is_none")]
    pub exclusive_maximum: Option<f64>,

    /// Minimum value (inclusive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,

    /// Exclusive minimum value
    #[serde(rename = "exclusiveMinimum", skip_serializing_if = "Option::is_none")]
    pub exclusive_minimum: Option<f64>,

    // String validation
    /// Maximum string length
    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u64>,

    /// Minimum string length
    #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u64>,

    /// String pattern (regex)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,

    // Array validation
    /// Array items schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<SchemaOrBool>>,

    /// Additional items schema (for tuple validation)
    #[serde(rename = "additionalItems", skip_serializing_if = "Option::is_none")]
    pub additional_items: Option<Box<SchemaOrBool>>,

    /// Maximum array length
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u64>,

    /// Minimum array length
    #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u64>,

    /// Unique items constraint
    #[serde(rename = "uniqueItems", skip_serializing_if = "Option::is_none")]
    pub unique_items: Option<bool>,

    /// Contains schema (at least one item must match)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains: Option<Box<Schema>>,

    // Object validation
    /// Maximum number of properties
    #[serde(rename = "maxProperties", skip_serializing_if = "Option::is_none")]
    pub max_properties: Option<u64>,

    /// Minimum number of properties
    #[serde(rename = "minProperties", skip_serializing_if = "Option::is_none")]
    pub min_properties: Option<u64>,

    /// Required properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Object properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, SchemaOrReference>>,

    /// Pattern properties
    #[serde(rename = "patternProperties", skip_serializing_if = "Option::is_none")]
    pub pattern_properties: Option<HashMap<String, SchemaOrReference>>,

    /// Additional properties schema
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<Box<SchemaOrBool>>,

    /// Dependencies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, SchemaOrStringArray>>,

    /// Property names schema
    #[serde(rename = "propertyNames", skip_serializing_if = "Option::is_none")]
    pub property_names: Option<Box<Schema>>,

    // Conditional schemas
    /// If schema
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_schema: Option<Box<Schema>>,

    /// Then schema
    #[serde(rename = "then", skip_serializing_if = "Option::is_none")]
    pub then_schema: Option<Box<Schema>>,

    /// Else schema
    #[serde(rename = "else", skip_serializing_if = "Option::is_none")]
    pub else_schema: Option<Box<Schema>>,

    // Schema composition
    /// All of (must match all schemas)
    #[serde(rename = "allOf", skip_serializing_if = "Option::is_none")]
    pub all_of: Option<Vec<SchemaOrReference>>,

    /// Any of (must match at least one schema)
    #[serde(rename = "anyOf", skip_serializing_if = "Option::is_none")]
    pub any_of: Option<Vec<SchemaOrReference>>,

    /// One of (must match exactly one schema)
    #[serde(rename = "oneOf", skip_serializing_if = "Option::is_none")]
    pub one_of: Option<Vec<SchemaOrReference>>,

    /// Not schema (must not match this schema)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not: Option<Box<SchemaOrReference>>,

    // String formats (JSON Schema Draft 7)
    /// String format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    // Schema metadata
    /// Content encoding
    #[serde(rename = "contentEncoding", skip_serializing_if = "Option::is_none")]
    pub content_encoding: Option<String>,

    /// Content media type
    #[serde(rename = "contentMediaType", skip_serializing_if = "Option::is_none")]
    pub content_media_type: Option<String>,

    /// Definitions (reusable schemas)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definitions: Option<HashMap<String, Schema>>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

/// JSON Schema types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    Null,
    Boolean,
    Object,
    Array,
    Number,
    String,
    Integer,
}

/// Schema or boolean value (for additionalProperties, additionalItems, etc.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SchemaOrBool {
    Schema(Box<Schema>),
    Bool(bool),
}

/// Schema or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SchemaOrReference {
    Schema(Box<Schema>),
    Reference(Reference),
}

/// Schema or array of strings (for dependencies)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SchemaOrStringArray {
    Schema(Box<Schema>),
    StringArray(Vec<String>),
}

impl Schema {
    /// Create a new empty schema
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a schema for any type
    pub fn any() -> Self {
        Self::new()
    }

    /// Create a boolean schema
    pub fn boolean() -> Self {
        Self::new().with_type(SchemaType::Boolean)
    }

    /// Create a string schema
    pub fn string() -> Self {
        Self::new().with_type(SchemaType::String)
    }

    /// Create a number schema
    pub fn number() -> Self {
        Self::new().with_type(SchemaType::Number)
    }

    /// Create an integer schema
    pub fn integer() -> Self {
        Self::new().with_type(SchemaType::Integer)
    }

    /// Create an array schema
    pub fn array() -> Self {
        Self::new().with_type(SchemaType::Array)
    }

    /// Create an object schema
    pub fn object() -> Self {
        Self::new().with_type(SchemaType::Object)
    }

    /// Create a null schema
    pub fn null() -> Self {
        Self::new().with_type(SchemaType::Null)
    }

    /// Set the schema type
    pub fn with_type(mut self, schema_type: SchemaType) -> Self {
        self.schema_type = Some(schema_type);
        self
    }

    /// Set the title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the default value
    pub fn with_default(mut self, default: impl Into<Value>) -> Self {
        self.default = Some(default.into());
        self
    }

    /// Set enum values
    pub fn with_enum(mut self, values: Vec<Value>) -> Self {
        self.enum_values = Some(values);
        self
    }

    /// Set array items schema
    pub fn with_items(mut self, items: Schema) -> Self {
        self.items = Some(Box::new(SchemaOrBool::Schema(Box::new(items))));
        self
    }

    /// Set object properties
    pub fn with_properties(mut self, properties: HashMap<String, SchemaOrReference>) -> Self {
        self.properties = Some(properties);
        self
    }

    /// Add a property to object schema
    pub fn with_property(mut self, name: impl Into<String>, schema: Schema) -> Self {
        if self.properties.is_none() {
            self.properties = Some(HashMap::new());
        }
        self.properties
            .as_mut()
            .unwrap()
            .insert(name.into(), SchemaOrReference::Schema(Box::new(schema)));
        self
    }

    /// Set required properties
    pub fn with_required(mut self, required: Vec<String>) -> Self {
        self.required = Some(required);
        self
    }

    /// Add a required property
    pub fn require_property(mut self, property: impl Into<String>) -> Self {
        if self.required.is_none() {
            self.required = Some(Vec::new());
        }
        self.required.as_mut().unwrap().push(property.into());
        self
    }

    /// Set minimum value
    pub fn with_minimum(mut self, minimum: f64) -> Self {
        self.minimum = Some(minimum);
        self
    }

    /// Set maximum value
    pub fn with_maximum(mut self, maximum: f64) -> Self {
        self.maximum = Some(maximum);
        self
    }

    /// Set minimum length
    pub fn with_min_length(mut self, min_length: u64) -> Self {
        self.min_length = Some(min_length);
        self
    }

    /// Set maximum length
    pub fn with_max_length(mut self, max_length: u64) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Set pattern
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.pattern = Some(pattern.into());
        self
    }

    /// Set format
    pub fn with_format(mut self, format: impl Into<String>) -> Self {
        self.format = Some(format.into());
        self
    }

    /// Set minimum array length
    pub fn with_min_items(mut self, min_items: u64) -> Self {
        self.min_items = Some(min_items);
        self
    }

    /// Set maximum array length
    pub fn with_max_items(mut self, max_items: u64) -> Self {
        self.max_items = Some(max_items);
        self
    }

    /// Add an extension field
    pub fn with_extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self {
            schema: None,
            id: None,
            reference: None,
            comment: None,
            title: None,
            description: None,
            default: None,
            examples: None,
            read_only: None,
            write_only: None,
            schema_type: None,
            enum_values: None,
            const_value: None,
            multiple_of: None,
            maximum: None,
            exclusive_maximum: None,
            minimum: None,
            exclusive_minimum: None,
            max_length: None,
            min_length: None,
            pattern: None,
            items: None,
            additional_items: None,
            max_items: None,
            min_items: None,
            unique_items: None,
            contains: None,
            max_properties: None,
            min_properties: None,
            required: None,
            properties: None,
            pattern_properties: None,
            additional_properties: None,
            dependencies: None,
            property_names: None,
            if_schema: None,
            then_schema: None,
            else_schema: None,
            all_of: None,
            any_of: None,
            one_of: None,
            not: None,
            format: None,
            content_encoding: None,
            content_media_type: None,
            definitions: None,
            extensions: Extensions::new(),
        }
    }
}

impl Validate for Schema {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate numeric constraints
        if let (Some(min), Some(max)) = (self.minimum, self.maximum)
            && min > max
        {
            return Err(crate::error::OpenRpcError::validation(
                "minimum cannot be greater than maximum",
            ));
        }

        if let (Some(min), Some(max)) = (self.min_length, self.max_length)
            && min > max
        {
            return Err(crate::error::OpenRpcError::validation(
                "minLength cannot be greater than maxLength",
            ));
        }

        if let (Some(min), Some(max)) = (self.min_items, self.max_items)
            && min > max
        {
            return Err(crate::error::OpenRpcError::validation(
                "minItems cannot be greater than maxItems",
            ));
        }

        if let (Some(min), Some(max)) = (self.min_properties, self.max_properties)
            && min > max
        {
            return Err(crate::error::OpenRpcError::validation(
                "minProperties cannot be greater than maxProperties",
            ));
        }

        // Validate multipleOf
        if let Some(multiple_of) = self.multiple_of
            && multiple_of <= 0.0
        {
            return Err(crate::error::OpenRpcError::validation(
                "multipleOf must be greater than 0",
            ));
        }

        // Validate pattern if present
        if let Some(ref pattern) = self.pattern {
            // Basic regex validation - would need regex crate for full validation
            if pattern.is_empty() {
                return Err(crate::error::OpenRpcError::validation(
                    "pattern cannot be empty",
                ));
            }
        }

        // Validate URL format for id
        if let Some(ref id) = self.id {
            crate::validation::validate_url(id)?;
        }

        // Validate items schema
        if let Some(ref items) = self.items {
            validate_schema_or_bool(items.as_ref())?;
        }

        if let Some(ref additional_items) = self.additional_items {
            validate_schema_or_bool(additional_items.as_ref())?;
        }

        if let Some(ref additional_properties) = self.additional_properties {
            validate_schema_or_bool(additional_properties.as_ref())?;
        }

        // Validate properties
        if let Some(ref properties) = self.properties {
            for (key, schema_or_ref) in properties {
                if key.is_empty() {
                    return Err(crate::error::OpenRpcError::validation(
                        "property name cannot be empty",
                    ));
                }
                validate_schema_or_reference(schema_or_ref)?;
            }
        }

        if let Some(ref pattern_properties) = self.pattern_properties {
            for (pattern, schema_or_ref) in pattern_properties {
                if pattern.is_empty() {
                    return Err(crate::error::OpenRpcError::validation(
                        "pattern property name cannot be empty",
                    ));
                }
                validate_schema_or_reference(schema_or_ref)?;
            }
        }

        if let Some(ref dependencies) = self.dependencies {
            for (property, dependency) in dependencies {
                if property.is_empty() {
                    return Err(crate::error::OpenRpcError::validation(
                        "dependency property name cannot be empty",
                    ));
                }
                validate_schema_dependency(dependency)?;
            }
        }

        // Validate composition schemas
        if let Some(ref all_of) = self.all_of {
            for schema_or_ref in all_of {
                validate_schema_or_reference(schema_or_ref)?;
            }
        }

        if let Some(ref any_of) = self.any_of {
            for schema_or_ref in any_of {
                validate_schema_or_reference(schema_or_ref)?;
            }
        }

        if let Some(ref one_of) = self.one_of {
            for schema_or_ref in one_of {
                validate_schema_or_reference(schema_or_ref)?;
            }
        }

        // Validate nested schemas
        if let Some(ref contains) = self.contains {
            contains.validate()?;
        }

        if let Some(ref property_names) = self.property_names {
            property_names.validate()?;
        }

        if let Some(ref if_schema) = self.if_schema {
            if_schema.validate()?;
        }

        if let Some(ref then_schema) = self.then_schema {
            then_schema.validate()?;
        }

        if let Some(ref else_schema) = self.else_schema {
            else_schema.validate()?;
        }

        if let Some(ref not) = self.not {
            validate_schema_or_reference(not.as_ref())?;
        }

        // Validate definitions
        if let Some(ref definitions) = self.definitions {
            for (key, schema) in definitions {
                crate::validation::validate_component_key(key)?;
                schema.validate()?;
            }
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

fn validate_schema_or_bool(schema_or_bool: &SchemaOrBool) -> OpenRpcResult<()> {
    match schema_or_bool {
        SchemaOrBool::Schema(schema) => schema.as_ref().validate(),
        SchemaOrBool::Bool(_) => Ok(()),
    }
}

fn validate_schema_or_reference(schema_or_ref: &SchemaOrReference) -> OpenRpcResult<()> {
    match schema_or_ref {
        SchemaOrReference::Schema(schema) => schema.as_ref().validate(),
        SchemaOrReference::Reference(reference) => reference.validate(),
    }
}

fn validate_schema_dependency(dependency: &SchemaOrStringArray) -> OpenRpcResult<()> {
    match dependency {
        SchemaOrStringArray::Schema(schema) => schema.as_ref().validate(),
        SchemaOrStringArray::StringArray(required_properties) => {
            if required_properties.iter().any(String::is_empty) {
                return Err(crate::error::OpenRpcError::validation(
                    "dependency property names cannot be empty",
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_schema_creation() {
        let schema = Schema::string()
            .with_title("Name")
            .with_description("A person's name")
            .with_min_length(1)
            .with_max_length(100);

        assert_eq!(schema.schema_type, Some(SchemaType::String));
        assert_eq!(schema.title, Some("Name".to_string()));
        assert_eq!(schema.min_length, Some(1));
        assert_eq!(schema.max_length, Some(100));
    }

    #[test]
    fn test_schema_types() {
        assert_eq!(Schema::boolean().schema_type, Some(SchemaType::Boolean));
        assert_eq!(Schema::string().schema_type, Some(SchemaType::String));
        assert_eq!(Schema::number().schema_type, Some(SchemaType::Number));
        assert_eq!(Schema::integer().schema_type, Some(SchemaType::Integer));
        assert_eq!(Schema::array().schema_type, Some(SchemaType::Array));
        assert_eq!(Schema::object().schema_type, Some(SchemaType::Object));
        assert_eq!(Schema::null().schema_type, Some(SchemaType::Null));
    }

    #[test]
    fn test_object_schema() {
        let schema = Schema::object()
            .with_property("name", Schema::string().with_min_length(1))
            .with_property("age", Schema::integer().with_minimum(0.0))
            .require_property("name");

        assert!(schema.properties.is_some());
        assert_eq!(schema.properties.as_ref().unwrap().len(), 2);
        assert_eq!(schema.required, Some(vec!["name".to_string()]));
    }

    #[test]
    fn test_array_schema() {
        let schema = Schema::array()
            .with_items(Schema::string())
            .with_min_items(1)
            .with_max_items(10);

        assert!(schema.items.is_some());
        assert_eq!(schema.min_items, Some(1));
        assert_eq!(schema.max_items, Some(10));
    }

    #[test]
    fn test_schema_validation() {
        // Valid schema
        let schema = Schema::string().with_min_length(1).with_max_length(10);
        assert!(schema.validate().is_ok());

        // Invalid - min > max
        let schema = Schema::string().with_min_length(10).with_max_length(1);
        assert!(schema.validate().is_err());

        // Invalid - multipleOf <= 0
        let mut schema = Schema::number();
        schema.multiple_of = Some(0.0);
        assert!(schema.validate().is_err());
    }

    #[test]
    fn test_schema_serialization() {
        let schema = Schema::string().with_title("Name").with_min_length(1);

        let json = serde_json::to_value(&schema).unwrap();
        let expected = json!({
            "type": "string",
            "title": "Name",
            "minLength": 1
        });

        assert_eq!(json, expected);

        let deserialized: Schema = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, schema);
    }

    #[test]
    fn test_schema_or_reference() {
        let schema_ref = SchemaOrReference::Reference(Reference::schema("User"));
        let schema_direct = SchemaOrReference::Schema(Box::new(Schema::string()));

        let schema_ref_json = serde_json::to_value(&schema_ref).unwrap();
        let schema_direct_json = serde_json::to_value(&schema_direct).unwrap();

        assert!(schema_ref_json.as_object().unwrap().contains_key("$ref"));
        assert!(schema_direct_json.as_object().unwrap().contains_key("type"));
    }

    #[test]
    fn test_schema_builder() {
        let schema = Schema::builder()
            .schema_type(SchemaType::String)
            .title("Test".to_string())
            .min_length(1)
            .build();

        assert_eq!(schema.schema_type, Some(SchemaType::String));
        assert_eq!(schema.title, Some("Test".to_string()));
        assert_eq!(schema.min_length, Some(1));
    }

    #[test]
    fn test_schema_with_extensions() {
        let schema = Schema::string().with_extension("x-custom", "value");

        assert!(!schema.extensions.is_empty());
        assert_eq!(schema.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn convenience_methods_cover_common_schema_fields() {
        let mut properties = HashMap::new();
        properties.insert(
            "manager".to_string(),
            SchemaOrReference::Reference(Reference::schema("User")),
        );

        let schema = Schema::any()
            .with_title("User")
            .with_description("A user object")
            .with_default(json!({"name": "Alice"}))
            .with_enum(vec![json!("admin"), json!("user")])
            .with_properties(properties)
            .with_required(vec!["name".to_string()])
            .require_property("email")
            .with_minimum(1.0)
            .with_maximum(10.0)
            .with_pattern("^[a-z]+$")
            .with_format("email");

        assert_eq!(schema.schema_type, None);
        assert_eq!(schema.title.as_deref(), Some("User"));
        assert_eq!(schema.description.as_deref(), Some("A user object"));
        assert_eq!(schema.default, Some(json!({"name": "Alice"})));
        assert_eq!(schema.enum_values.as_ref().unwrap().len(), 2);
        assert_eq!(schema.properties.as_ref().unwrap().len(), 1);
        assert_eq!(
            schema.required,
            Some(vec!["name".to_string(), "email".to_string()])
        );
        assert_eq!(schema.minimum, Some(1.0));
        assert_eq!(schema.maximum, Some(10.0));
        assert_eq!(schema.pattern.as_deref(), Some("^[a-z]+$"));
        assert_eq!(schema.format.as_deref(), Some("email"));
    }

    #[test]
    fn validate_accepts_nested_schema_containers() {
        let valid_nested = Schema::string().with_min_length(1);
        let mut pattern_properties = HashMap::new();
        pattern_properties.insert(
            "^x-".to_string(),
            SchemaOrReference::Schema(Box::new(valid_nested.clone())),
        );
        let mut dependencies = HashMap::new();
        dependencies.insert(
            "credit_card".to_string(),
            SchemaOrStringArray::StringArray(vec!["billing_address".to_string()]),
        );
        dependencies.insert(
            "billing_address".to_string(),
            SchemaOrStringArray::Schema(Box::new(Schema::object())),
        );
        let mut definitions = HashMap::new();
        definitions.insert("Address".to_string(), Schema::object());

        let mut schema = Schema::object()
            .with_property("name", valid_nested.clone())
            .with_items(valid_nested.clone());
        schema.additional_items = Some(Box::new(SchemaOrBool::Bool(false)));
        schema.additional_properties = Some(Box::new(SchemaOrBool::Schema(Box::new(
            valid_nested.clone(),
        ))));
        schema.pattern_properties = Some(pattern_properties);
        schema.dependencies = Some(dependencies);
        schema.contains = Some(Box::new(valid_nested.clone()));
        schema.property_names = Some(Box::new(Schema::string()));
        schema.if_schema = Some(Box::new(Schema::object()));
        schema.then_schema = Some(Box::new(Schema::object()));
        schema.else_schema = Some(Box::new(Schema::object()));
        schema.all_of = Some(vec![SchemaOrReference::Schema(Box::new(Schema::object()))]);
        schema.any_of = Some(vec![SchemaOrReference::Reference(Reference::schema(
            "Address",
        ))]);
        schema.one_of = Some(vec![SchemaOrReference::Schema(Box::new(Schema::string()))]);
        schema.not = Some(Box::new(SchemaOrReference::Schema(
            Box::new(Schema::null()),
        )));
        schema.definitions = Some(definitions);

        assert!(schema.validate().is_ok());
    }

    #[test]
    fn validate_rejects_invalid_nested_schema_containers() {
        let invalid_nested = Schema::string().with_min_length(10).with_max_length(1);

        let mut schema = Schema::array();
        schema.additional_items = Some(Box::new(SchemaOrBool::Schema(Box::new(
            invalid_nested.clone(),
        ))));
        assert!(schema.validate().is_err());

        let mut schema = Schema::object();
        schema.additional_properties = Some(Box::new(SchemaOrBool::Schema(Box::new(
            invalid_nested.clone(),
        ))));
        assert!(schema.validate().is_err());

        let mut pattern_properties = HashMap::new();
        pattern_properties.insert(
            "".to_string(),
            SchemaOrReference::Schema(Box::new(Schema::string())),
        );
        let mut schema = Schema::object();
        schema.pattern_properties = Some(pattern_properties);
        assert!(schema.validate().is_err());

        let mut dependencies = HashMap::new();
        dependencies.insert(
            "".to_string(),
            SchemaOrStringArray::StringArray(vec!["name".to_string()]),
        );
        let mut schema = Schema::object();
        schema.dependencies = Some(dependencies);
        assert!(schema.validate().is_err());

        let mut dependencies = HashMap::new();
        dependencies.insert(
            "name".to_string(),
            SchemaOrStringArray::StringArray(vec!["".to_string()]),
        );
        let mut schema = Schema::object();
        schema.dependencies = Some(dependencies);
        assert!(schema.validate().is_err());

        let mut schema = Schema::object();
        schema.all_of = Some(vec![SchemaOrReference::Schema(Box::new(
            invalid_nested.clone(),
        ))]);
        assert!(schema.validate().is_err());

        let mut schema = Schema::object();
        schema.not = Some(Box::new(SchemaOrReference::Reference(Reference::new(
            "#/components/schemas/invalid name",
        ))));
        assert!(schema.validate().is_err());
    }
}
