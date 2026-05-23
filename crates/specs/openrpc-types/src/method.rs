//! Method Object for OpenRPC specification.

use crate::{
    ContentDescriptor, ErrorObject, ExamplePairing, Extensions, ExternalDocumentation, Link,
    Reference, Server, Tag,
    error::OpenRpcResult,
    validation::{Validate, ValidateUnique},
};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// Describes the interface for the given method name.
/// The method name is used as the method field of the JSON-RPC body.
/// It therefore MUST be unique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
pub struct Method {
    /// The canonical name for the method.
    /// The name MUST be unique within the methods array.
    pub name: String,

    /// A list of tags for API documentation control.
    /// Tags can be used for logical grouping of methods by resources or any other qualifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<TagOrReference>>,

    /// A short summary of what the method does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A verbose explanation of the method behavior.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional external documentation for this method.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocumentation>,

    /// A list of parameters that are applicable for this method.
    /// The list MUST NOT include duplicated parameters and therefore require name to be unique.
    /// The list can use the Reference Object to link to parameters that are defined by the
    /// Content Descriptor Object. All optional params (content descriptor objects with
    /// "required": false) MUST be positioned after all required params in the list.
    pub params: Vec<ContentDescriptorOrReference>,

    /// The description of the result returned by the method.
    /// If defined, it MUST be a Content Descriptor or Reference Object.
    /// If undefined, the method MUST only be used as a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ContentDescriptorOrReference>,

    /// Declares this method to be deprecated.
    /// Consumers SHOULD refrain from usage of the declared method.
    /// Default value is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// An alternative servers array to service this method.
    /// If an alternative servers array is specified at the Root level,
    /// it will be overridden by this value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<Vec<Server>>,

    /// A list of custom application defined errors that MAY be returned.
    /// The Errors MUST have unique error codes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<ErrorOrReference>>,

    /// A list of possible links from this method call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<LinkOrReference>>,

    /// The expected format of the parameters.
    /// As per the JSON-RPC 2.0 specification, the params of a JSON-RPC request object
    /// may be an array, object, or either (represented as by-position, by-name, and either respectively).
    /// Defaults to "either".
    #[serde(rename = "paramStructure", skip_serializing_if = "Option::is_none")]
    pub param_structure: Option<ParameterStructure>,

    /// Array of Example Pairing Objects where each example includes a valid
    /// params-to-result Content Descriptor pairing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<ExamplePairingOrReference>>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

/// Tag or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TagOrReference {
    Tag(Tag),
    Reference(Reference),
}

/// Content Descriptor or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentDescriptorOrReference {
    ContentDescriptor(Box<ContentDescriptor>),
    Reference(Reference),
}

/// Error Object or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ErrorOrReference {
    Error(ErrorObject),
    Reference(Reference),
}

/// Link Object or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LinkOrReference {
    Link(Box<Link>),
    Reference(Reference),
}

/// Example Pairing Object or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExamplePairingOrReference {
    ExamplePairing(Box<ExamplePairing>),
    Reference(Reference),
}

/// Parameter structure enum values
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParameterStructure {
    ByName,
    ByPosition,
    Either,
}

impl Method {
    /// Create a new Method with required fields
    pub fn new(name: impl Into<String>, params: Vec<ContentDescriptorOrReference>) -> Self {
        Self {
            name: name.into(),
            tags: None,
            summary: None,
            description: None,
            external_docs: None,
            params,
            result: None,
            deprecated: None,
            servers: None,
            errors: None,
            links: None,
            param_structure: None,
            examples: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the tags
    pub fn with_tags(mut self, tags: Vec<TagOrReference>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: TagOrReference) -> Self {
        if self.tags.is_none() {
            self.tags = Some(Vec::new());
        }
        self.tags.as_mut().unwrap().push(tag);
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

    /// Set the external documentation
    pub fn with_external_docs(mut self, external_docs: ExternalDocumentation) -> Self {
        self.external_docs = Some(external_docs);
        self
    }

    /// Set the result
    pub fn with_result(mut self, result: ContentDescriptorOrReference) -> Self {
        self.result = Some(result);
        self
    }

    /// Set whether this method is deprecated
    pub fn with_deprecated(mut self, deprecated: bool) -> Self {
        self.deprecated = Some(deprecated);
        self
    }

    /// Mark this method as deprecated
    pub fn deprecated(mut self) -> Self {
        self.deprecated = Some(true);
        self
    }

    /// Set the servers
    pub fn with_servers(mut self, servers: Vec<Server>) -> Self {
        self.servers = Some(servers);
        self
    }

    /// Add a server
    pub fn with_server(mut self, server: Server) -> Self {
        if self.servers.is_none() {
            self.servers = Some(Vec::new());
        }
        self.servers.as_mut().unwrap().push(server);
        self
    }

    /// Set the errors
    pub fn with_errors(mut self, errors: Vec<ErrorOrReference>) -> Self {
        self.errors = Some(errors);
        self
    }

    /// Add an error
    pub fn with_error(mut self, error: ErrorOrReference) -> Self {
        if self.errors.is_none() {
            self.errors = Some(Vec::new());
        }
        self.errors.as_mut().unwrap().push(error);
        self
    }

    /// Set the links
    pub fn with_links(mut self, links: Vec<LinkOrReference>) -> Self {
        self.links = Some(links);
        self
    }

    /// Add a link
    pub fn with_link(mut self, link: LinkOrReference) -> Self {
        if self.links.is_none() {
            self.links = Some(Vec::new());
        }
        self.links.as_mut().unwrap().push(link);
        self
    }

    /// Set the parameter structure
    pub fn with_param_structure(mut self, param_structure: ParameterStructure) -> Self {
        self.param_structure = Some(param_structure);
        self
    }

    /// Set parameter structure to by-name
    pub fn by_name(mut self) -> Self {
        self.param_structure = Some(ParameterStructure::ByName);
        self
    }

    /// Set parameter structure to by-position
    pub fn by_position(mut self) -> Self {
        self.param_structure = Some(ParameterStructure::ByPosition);
        self
    }

    /// Set the examples
    pub fn with_examples(mut self, examples: Vec<ExamplePairingOrReference>) -> Self {
        self.examples = Some(examples);
        self
    }

    /// Add an example
    pub fn with_example(mut self, example: ExamplePairingOrReference) -> Self {
        if self.examples.is_none() {
            self.examples = Some(Vec::new());
        }
        self.examples.as_mut().unwrap().push(example);
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

    /// Check if this method is deprecated (defaults to false)
    pub fn is_deprecated(&self) -> bool {
        self.deprecated.unwrap_or(false)
    }

    /// Check if this method is a notification (has no result)
    pub fn is_notification(&self) -> bool {
        self.result.is_none()
    }

    /// Get the parameter structure (defaults to Either)
    pub fn get_param_structure(&self) -> ParameterStructure {
        self.param_structure
            .clone()
            .unwrap_or(ParameterStructure::Either)
    }
}

impl Validate for Method {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }
        crate::validation::validate_method_name(&self.name)?;

        // Validate parameter names are unique
        self.params.validate_unique(
            |param| match param {
                ContentDescriptorOrReference::ContentDescriptor(cd) => cd.as_ref().name.clone(),
                ContentDescriptorOrReference::Reference(r) => r.reference.clone(),
            },
            "method parameters",
        )?;

        // Validate parameters
        for (i, param) in self.params.iter().enumerate() {
            param.validate().map_err(|e| {
                crate::error::OpenRpcError::validation_with_path(
                    e.to_string(),
                    format!("params[{}]", i),
                )
            })?;
        }

        // Validate parameter ordering: required params must come before optional
        let mut seen_optional = false;
        for param in &self.params {
            if let ContentDescriptorOrReference::ContentDescriptor(cd) = param {
                if cd.as_ref().is_required() {
                    if seen_optional {
                        return Err(crate::error::OpenRpcError::validation(
                            "required parameters must be positioned before optional parameters",
                        ));
                    }
                } else {
                    seen_optional = true;
                }
            }
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

        // Validate tags if present
        if let Some(ref tags) = self.tags {
            for (i, tag) in tags.iter().enumerate() {
                tag.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("tags[{}]", i),
                    )
                })?;
            }
        }

        // Validate external docs if present
        if let Some(ref external_docs) = self.external_docs {
            external_docs.validate()?;
        }

        // Validate servers if present
        if let Some(ref servers) = self.servers {
            for (i, server) in servers.iter().enumerate() {
                server.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("servers[{}]", i),
                    )
                })?;
            }
        }

        // Validate errors if present
        if let Some(ref errors) = self.errors {
            // Error codes must be unique
            errors.validate_unique(
                |error| match error {
                    ErrorOrReference::Error(e) => e.code.to_string(),
                    ErrorOrReference::Reference(r) => r.reference.clone(),
                },
                "method errors",
            )?;

            for (i, error) in errors.iter().enumerate() {
                error.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("errors[{}]", i),
                    )
                })?;
            }
        }

        // Validate links if present
        if let Some(ref links) = self.links {
            for (i, link) in links.iter().enumerate() {
                link.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("links[{}]", i),
                    )
                })?;
            }
        }

        // Validate examples if present
        if let Some(ref examples) = self.examples {
            for (i, example) in examples.iter().enumerate() {
                example.validate().map_err(|e| {
                    crate::error::OpenRpcError::validation_with_path(
                        e.to_string(),
                        format!("examples[{}]", i),
                    )
                })?;
            }
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

// Implement validation for all the union types
impl Validate for TagOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            TagOrReference::Tag(tag) => tag.validate(),
            TagOrReference::Reference(reference) => reference.validate(),
        }
    }
}

impl Validate for ContentDescriptorOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            ContentDescriptorOrReference::ContentDescriptor(cd) => cd.as_ref().validate(),
            ContentDescriptorOrReference::Reference(reference) => reference.validate(),
        }
    }
}

impl Validate for ErrorOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            ErrorOrReference::Error(error) => error.validate(),
            ErrorOrReference::Reference(reference) => reference.validate(),
        }
    }
}

impl Validate for LinkOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            LinkOrReference::Link(link) => link.as_ref().validate(),
            LinkOrReference::Reference(reference) => reference.validate(),
        }
    }
}

impl Validate for ExamplePairingOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            ExamplePairingOrReference::ExamplePairing(example) => example.as_ref().validate(),
            ExamplePairingOrReference::Reference(reference) => reference.validate(),
        }
    }
}

// Convenience From implementations
impl From<ContentDescriptor> for ContentDescriptorOrReference {
    fn from(cd: ContentDescriptor) -> Self {
        ContentDescriptorOrReference::ContentDescriptor(Box::new(cd))
    }
}

impl From<Reference> for ContentDescriptorOrReference {
    fn from(reference: Reference) -> Self {
        ContentDescriptorOrReference::Reference(reference)
    }
}

impl From<ErrorObject> for ErrorOrReference {
    fn from(error: ErrorObject) -> Self {
        ErrorOrReference::Error(error)
    }
}

impl From<Reference> for ErrorOrReference {
    fn from(reference: Reference) -> Self {
        ErrorOrReference::Reference(reference)
    }
}

impl From<Link> for LinkOrReference {
    fn from(link: Link) -> Self {
        LinkOrReference::Link(Box::new(link))
    }
}

impl From<Reference> for LinkOrReference {
    fn from(reference: Reference) -> Self {
        LinkOrReference::Reference(reference)
    }
}

impl From<Tag> for TagOrReference {
    fn from(tag: Tag) -> Self {
        TagOrReference::Tag(tag)
    }
}

impl From<Reference> for TagOrReference {
    fn from(reference: Reference) -> Self {
        TagOrReference::Reference(reference)
    }
}

impl From<ExamplePairing> for ExamplePairingOrReference {
    fn from(example: ExamplePairing) -> Self {
        ExamplePairingOrReference::ExamplePairing(Box::new(example))
    }
}

impl From<Reference> for ExamplePairingOrReference {
    fn from(reference: Reference) -> Self {
        ExamplePairingOrReference::Reference(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_method_creation() {
        let params = vec![
            ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("username", crate::Schema::string()).required(),
            )),
            ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("age", crate::Schema::integer()).optional(),
            )),
        ];

        let method = Method::new("createUser", params)
            .with_summary("Create a new user")
            .with_result(ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("user", crate::Schema::object()),
            )));

        assert_eq!(method.name, "createUser");
        assert_eq!(method.params.len(), 2);
        assert!(method.result.is_some());
        assert!(!method.is_notification());
    }

    #[test]
    fn test_method_notification() {
        let method = Method::new(
            "logMessage",
            vec![ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("message", crate::Schema::string()),
            ))],
        );

        assert!(method.is_notification());
    }

    #[test]
    fn test_parameter_structure() {
        let method = Method::new("test", vec![]).by_name();
        assert_eq!(method.get_param_structure(), ParameterStructure::ByName);

        let method = Method::new("test", vec![]).by_position();
        assert_eq!(method.get_param_structure(), ParameterStructure::ByPosition);

        let method = Method::new("test", vec![]);
        assert_eq!(method.get_param_structure(), ParameterStructure::Either);
    }

    #[test]
    fn test_method_validation() {
        // Valid method
        let params = vec![ContentDescriptorOrReference::ContentDescriptor(Box::new(
            ContentDescriptor::new("param1", crate::Schema::string()),
        ))];
        let method = Method::new("validMethod", params);
        assert!(method.validate().is_ok());

        // Invalid - empty name
        let method = Method::new("", vec![]);
        assert!(method.validate().is_err());

        // Invalid - reserved method name
        let method = Method::new("rpc.custom", vec![]);
        assert!(method.validate().is_err());
    }

    #[test]
    fn test_parameter_ordering_validation() {
        // Valid - required before optional
        let params = vec![
            ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("required", crate::Schema::string()).required(),
            )),
            ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("optional", crate::Schema::string()).optional(),
            )),
        ];
        let method = Method::new("test", params);
        assert!(method.validate().is_ok());

        // Invalid - optional before required
        let params = vec![
            ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("optional", crate::Schema::string()).optional(),
            )),
            ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("required", crate::Schema::string()).required(),
            )),
        ];
        let method = Method::new("test", params);
        assert!(method.validate().is_err());
    }

    #[test]
    fn test_method_builder() {
        let method = Method::builder()
            .name("testMethod".to_string())
            .params(vec![])
            .summary("Test method".to_string())
            .deprecated(true)
            .build();

        assert_eq!(method.name, "testMethod");
        assert_eq!(method.summary, Some("Test method".to_string()));
        assert!(method.is_deprecated());
    }

    #[test]
    fn test_method_serialization() {
        let method = Method::new(
            "getUser",
            vec![ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("id", crate::Schema::string()),
            ))],
        )
        .with_summary("Get user by ID");

        let json_value = serde_json::to_value(&method).unwrap();

        assert_eq!(json_value["name"], "getUser");
        assert_eq!(json_value["summary"], "Get user by ID");
        assert!(json_value["params"].is_array());
        assert_eq!(json_value["params"].as_array().unwrap().len(), 1);

        let deserialized: Method = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, method);
    }

    #[test]
    fn test_parameter_structure_serialization() {
        assert_eq!(
            serde_json::to_value(&ParameterStructure::ByName).unwrap(),
            json!("by-name")
        );
        assert_eq!(
            serde_json::to_value(&ParameterStructure::ByPosition).unwrap(),
            json!("by-position")
        );
        assert_eq!(
            serde_json::to_value(&ParameterStructure::Either).unwrap(),
            json!("either")
        );
    }

    #[test]
    fn test_union_types() {
        // Test ContentDescriptorOrReference
        let cd_variant = ContentDescriptorOrReference::ContentDescriptor(Box::new(
            ContentDescriptor::new("test", crate::Schema::string()),
        ));
        let ref_variant =
            ContentDescriptorOrReference::Reference(Reference::content_descriptor("TestParam"));

        assert!(cd_variant.validate().is_ok());
        assert!(ref_variant.validate().is_ok());

        // Test serialization
        let cd_json = serde_json::to_value(&cd_variant).unwrap();
        assert!(cd_json["name"] == "test");

        let ref_json = serde_json::to_value(&ref_variant).unwrap();
        assert!(ref_json["$ref"] == "#/components/contentDescriptors/TestParam");
    }

    #[test]
    fn test_method_with_extensions() {
        let method = Method::new("test", vec![]).with_extension("x-custom", "value");

        assert!(!method.extensions.is_empty());
        assert_eq!(method.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn test_method_with_all_features() {
        let method = Method::new(
            "complexMethod",
            vec![ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("param1", crate::Schema::string()).required(),
            ))],
        )
        .with_tag(TagOrReference::Tag(Tag::new("user")))
        .with_summary("Complex method")
        .with_description("A complex method with all features")
        .with_result(ContentDescriptorOrReference::ContentDescriptor(Box::new(
            ContentDescriptor::new("result", crate::Schema::object()),
        )))
        .with_error(ErrorOrReference::Error(ErrorObject::new(
            1000,
            "Custom error",
        )))
        .with_link(LinkOrReference::Link(Box::new(Link::new("relatedMethod"))))
        .by_name()
        .deprecated();

        assert!(method.validate().is_ok());
        assert!(method.is_deprecated());
        assert_eq!(method.get_param_structure(), ParameterStructure::ByName);
        assert!(method.tags.is_some());
        assert!(method.errors.is_some());
        assert!(method.links.is_some());
    }

    #[test]
    fn helper_methods_set_and_append_optional_collections() {
        let method = Method::new("complexMethod", vec![])
            .with_tags(vec![Tag::new("users").into()])
            .with_tag(Reference::tag("AdminMethods").into())
            .with_external_docs(ExternalDocumentation::new(
                "https://docs.example.com/methods",
            ))
            .with_deprecated(false)
            .with_servers(vec![Server::new("primary", "https://api.example.com")])
            .with_server(Server::new("backup", "https://backup.example.com"))
            .with_errors(vec![ErrorObject::new(1000, "Quota exceeded").into()])
            .with_error(Reference::error("UserNotFound").into())
            .with_links(vec![Link::new("getUser").into()])
            .with_link(Reference::link("AuditLog").into())
            .with_param_structure(ParameterStructure::ByPosition)
            .with_examples(vec![ExamplePairing::new("created", vec![]).into()])
            .with_example(Reference::example_pairing("failed").into());

        assert!(method.validate().is_ok());
        assert!(!method.is_deprecated());
        assert_eq!(method.get_param_structure(), ParameterStructure::ByPosition);
        assert_eq!(method.tags.as_ref().unwrap().len(), 2);
        assert_eq!(method.servers.as_ref().unwrap().len(), 2);
        assert_eq!(method.errors.as_ref().unwrap().len(), 2);
        assert_eq!(method.links.as_ref().unwrap().len(), 2);
        assert_eq!(method.examples.as_ref().unwrap().len(), 2);
        assert!(method.external_docs.is_some());
    }

    #[test]
    fn distinct_error_references_do_not_collide_during_validation() {
        let method = Method::new("methodWithReferencedErrors", vec![]).with_errors(vec![
            Reference::error("UserNotFound").into(),
            Reference::error("RateLimited").into(),
        ]);

        assert!(method.validate().is_ok());

        let duplicate = Method::new("methodWithDuplicateErrorReference", vec![]).with_errors(vec![
            Reference::error("UserNotFound").into(),
            Reference::error("UserNotFound").into(),
        ]);

        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn nested_validation_errors_keep_their_field_path() {
        let method = Method::new("methodWithInvalidResult", vec![]).with_result(
            Reference {
                reference: String::new(),
            }
            .into(),
        );

        let error = method.validate().unwrap_err();
        assert_eq!(
            error.to_string(),
            "Validation error at result: Validation error: Reference string cannot be empty"
        );
    }
}
