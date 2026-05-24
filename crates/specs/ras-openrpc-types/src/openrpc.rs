//! OpenRPC Object - the root object of the OpenRPC specification.

use crate::{
    Components, Extensions, ExternalDocumentation, Info, Method, Reference, Server,
    error::OpenRpcResult,
    validation::{Validate, ValidateUnique},
};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// This is the root object of the OpenRPC document.
/// The contents of this object represent a whole OpenRPC document.
/// How this object is constructed or stored is outside the scope of the OpenRPC Specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct OpenRpc {
    /// This string MUST be the semantic version number of the OpenRPC Specification version
    /// that the OpenRPC document uses. The openrpc field SHOULD be used by tooling
    /// specifications and clients to interpret the OpenRPC document.
    /// This is not related to the API info.version string.
    pub openrpc: String,

    /// Provides metadata about the API. The metadata MAY be used by tooling as required.
    pub info: Info,

    /// An array of Server Objects, which provide connectivity information to a target server.
    /// If the servers property is not provided, or is an empty array, the default value
    /// would be a Server Object with a url value of localhost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<Vec<Server>>,

    /// The available methods for the API. While it is required, the array may be empty
    /// (to handle security filtering, for example).
    pub methods: Vec<MethodOrReference>,

    /// An element to hold various schemas for the specification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Components>,

    /// Additional external documentation.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocumentation>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

/// Method Object or Reference Object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MethodOrReference {
    Method(Box<Method>),
    Reference(Reference),
}

impl OpenRpc {
    /// Create a new OpenRPC document with required fields
    pub fn new(
        openrpc_version: impl Into<String>,
        info: Info,
        methods: Vec<MethodOrReference>,
    ) -> Self {
        Self {
            openrpc: openrpc_version.into(),
            info,
            servers: None,
            methods,
            components: None,
            external_docs: None,
            extensions: Extensions::new(),
        }
    }

    /// Create a new OpenRPC document with the current specification version
    pub fn v1_3_2(info: Info, methods: Vec<MethodOrReference>) -> Self {
        Self::new(crate::version::CURRENT, info, methods)
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

    /// Set the components
    pub fn with_components(mut self, components: Components) -> Self {
        self.components = Some(components);
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

    /// Get the default server if no servers are specified
    pub fn get_default_server() -> Server {
        Server::new("default", "localhost")
    }

    /// Get the effective servers (returns default if none specified)
    pub fn get_servers(&self) -> Vec<&Server> {
        match &self.servers {
            Some(servers) if !servers.is_empty() => servers.iter().collect(),
            _ => vec![], // Return empty vec, caller should handle default
        }
    }

    /// Check if this document uses a supported OpenRPC version
    pub fn is_supported_version(&self) -> bool {
        crate::version::is_supported(&self.openrpc)
    }

    /// Get all method names (for uniqueness checking)
    pub fn get_method_names(&self) -> Vec<String> {
        self.methods
            .iter()
            .filter_map(|method| {
                match method {
                    MethodOrReference::Method(m) => Some(m.as_ref().name.clone()),
                    MethodOrReference::Reference(_) => None, // Can't extract name from reference
                }
            })
            .collect()
    }
}

impl Validate for OpenRpc {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate OpenRPC version
        crate::validation::validate_openrpc_version(&self.openrpc)?;

        // Validate info
        self.info.validate().map_err(|e| {
            crate::error::OpenRpcError::validation_with_path(e.to_string(), "info".to_string())
        })?;

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

        // Validate methods
        for (i, method) in self.methods.iter().enumerate() {
            method.validate().map_err(|e| {
                crate::error::OpenRpcError::validation_with_path(
                    e.to_string(),
                    format!("methods[{}]", i),
                )
            })?;
        }

        // Validate method names are unique
        let method_names: Vec<String> = self
            .methods
            .iter()
            .map(|method| match method {
                MethodOrReference::Method(m) => m.as_ref().name.clone(),
                MethodOrReference::Reference(r) => r.reference.clone(),
            })
            .collect();

        method_names.validate_unique(|name| name.clone(), "methods")?;

        // Validate components if present
        if let Some(ref components) = self.components {
            components.validate().map_err(|e| {
                crate::error::OpenRpcError::validation_with_path(
                    e.to_string(),
                    "components".to_string(),
                )
            })?;
        }

        // Validate external docs if present
        if let Some(ref external_docs) = self.external_docs {
            external_docs.validate().map_err(|e| {
                crate::error::OpenRpcError::validation_with_path(
                    e.to_string(),
                    "externalDocs".to_string(),
                )
            })?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

impl Validate for MethodOrReference {
    fn validate(&self) -> OpenRpcResult<()> {
        match self {
            MethodOrReference::Method(method) => method.as_ref().validate(),
            MethodOrReference::Reference(reference) => reference.validate(),
        }
    }
}

// Convenience From implementations
impl From<Method> for MethodOrReference {
    fn from(method: Method) -> Self {
        MethodOrReference::Method(Box::new(method))
    }
}

impl From<Reference> for MethodOrReference {
    fn from(reference: Reference) -> Self {
        MethodOrReference::Reference(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentDescriptor, Schema};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_openrpc_creation() {
        let info = Info::new("Test API", "1.0.0");
        let methods = vec![MethodOrReference::Method(Box::new(
            Method::new("test", vec![]).with_summary("Test method"),
        ))];

        let openrpc = OpenRpc::new("1.3.2", info, methods);

        assert_eq!(openrpc.openrpc, "1.3.2");
        assert_eq!(openrpc.info.title, "Test API");
        assert_eq!(openrpc.methods.len(), 1);
        assert!(openrpc.is_supported_version());
    }

    #[test]
    fn test_openrpc_v1_3_2() {
        let info = Info::new("Test API", "1.0.0");
        let methods = vec![];

        let openrpc = OpenRpc::v1_3_2(info, methods);

        assert_eq!(openrpc.openrpc, crate::version::CURRENT);
        assert!(openrpc.is_supported_version());
    }

    #[test]
    fn test_openrpc_with_servers() {
        let info = Info::new("Test API", "1.0.0");
        let server = Server::new("api", "https://api.example.com");

        let openrpc = OpenRpc::v1_3_2(info, vec![]).with_server(server.clone());

        assert!(openrpc.servers.is_some());
        assert_eq!(openrpc.servers.as_ref().unwrap().len(), 1);
        assert_eq!(openrpc.get_servers().len(), 1);
    }

    #[test]
    fn test_openrpc_validation() {
        let info = Info::new("Test API", "1.0.0");
        let methods = vec![MethodOrReference::Method(Box::new(Method::new(
            "validMethod",
            vec![],
        )))];

        // Valid document
        let openrpc = OpenRpc::new("1.3.2", info, methods);
        assert!(openrpc.validate().is_ok());

        // Invalid - unsupported version
        let info = Info::new("Test API", "1.0.0");
        let openrpc = OpenRpc::new("2.0.0", info, vec![]);
        assert!(openrpc.validate().is_err());

        // Invalid - duplicate method names
        let info = Info::new("Test API", "1.0.0");
        let methods = vec![
            MethodOrReference::Method(Box::new(Method::new("duplicate", vec![]))),
            MethodOrReference::Method(Box::new(Method::new("duplicate", vec![]))),
        ];
        let openrpc = OpenRpc::new("1.3.2", info, methods);
        assert!(openrpc.validate().is_err());
    }

    #[test]
    fn test_method_names_extraction() {
        let methods = vec![
            MethodOrReference::Method(Box::new(Method::new("method1", vec![]))),
            MethodOrReference::Method(Box::new(Method::new("method2", vec![]))),
            MethodOrReference::Reference(Reference::new("#/components/methods/method3")),
        ];

        let info = Info::new("Test", "1.0.0");
        let openrpc = OpenRpc::v1_3_2(info, methods);

        let names = openrpc.get_method_names();
        assert_eq!(names.len(), 2); // Only direct methods, not references
        assert!(names.contains(&"method1".to_string()));
        assert!(names.contains(&"method2".to_string()));
    }

    #[test]
    fn test_openrpc_builder() {
        let info = Info::new("Test API", "1.0.0");

        let openrpc = OpenRpc::builder()
            .openrpc(crate::version::CURRENT.to_string())
            .info(info)
            .methods(vec![])
            .build();

        assert_eq!(openrpc.openrpc, crate::version::CURRENT);
        assert!(openrpc.is_supported_version());
    }

    #[test]
    fn test_openrpc_serialization() {
        let info = Info::new("Test API", "1.0.0");
        let method = Method::new(
            "getUser",
            vec![
                crate::method::ContentDescriptorOrReference::ContentDescriptor(Box::new(
                    ContentDescriptor::new("id", Schema::string()),
                )),
            ],
        );

        let openrpc = OpenRpc::v1_3_2(info, vec![MethodOrReference::Method(Box::new(method))]);

        let json_value = serde_json::to_value(&openrpc).unwrap();

        assert_eq!(json_value["openrpc"], crate::version::CURRENT);
        assert_eq!(json_value["info"]["title"], "Test API");
        assert!(json_value["methods"].is_array());
        assert_eq!(json_value["methods"].as_array().unwrap().len(), 1);

        let deserialized: OpenRpc = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, openrpc);
    }

    #[test]
    fn test_method_or_reference_serialization() {
        // Test method variant
        let method_variant = MethodOrReference::Method(Box::new(Method::new("test", vec![])));
        let json = serde_json::to_value(&method_variant).unwrap();
        assert!(json["name"] == "test");

        // Test reference variant
        let ref_variant = MethodOrReference::Reference(Reference::new("#/components/methods/Test"));
        let json = serde_json::to_value(&ref_variant).unwrap();
        assert!(json["$ref"] == "#/components/methods/Test");
    }

    #[test]
    fn test_openrpc_with_components() {
        let info = Info::new("Test API", "1.0.0");
        let components = Components::new().with_schema("UserSchema", Schema::object());

        let openrpc = OpenRpc::v1_3_2(info, vec![]).with_components(components);

        assert!(openrpc.components.is_some());
        assert!(
            openrpc
                .components
                .as_ref()
                .unwrap()
                .get_schema("UserSchema")
                .is_some()
        );
    }

    #[test]
    fn test_openrpc_with_extensions() {
        let info = Info::new("Test API", "1.0.0");
        let openrpc = OpenRpc::v1_3_2(info, vec![]).with_extension("x-custom", "value");

        assert!(!openrpc.extensions.is_empty());
        assert_eq!(openrpc.extensions.get("x-custom"), Some(&json!("value")));
    }

    #[test]
    fn test_openrpc_complete_example() {
        let info = Info::new("Complete API", "1.0.0").with_description("A complete example API");

        let server = Server::new("production", "https://api.example.com");

        let user_param = ContentDescriptor::new(
            "user",
            Schema::object()
                .with_property("name", Schema::string())
                .with_property("email", Schema::string()),
        )
        .required();

        let method = Method::new(
            "createUser",
            vec![
                crate::method::ContentDescriptorOrReference::ContentDescriptor(Box::new(
                    user_param,
                )),
            ],
        )
        .with_summary("Create a new user")
        .with_result(
            crate::method::ContentDescriptorOrReference::ContentDescriptor(Box::new(
                ContentDescriptor::new("userId", Schema::string()),
            )),
        );

        let components = Components::new().with_schema(
            "User",
            Schema::object()
                .with_property("id", Schema::string())
                .with_property("name", Schema::string())
                .with_property("email", Schema::string()),
        );

        let openrpc = OpenRpc::v1_3_2(info, vec![MethodOrReference::Method(Box::new(method))])
            .with_server(server)
            .with_components(components);

        assert!(openrpc.validate().is_ok());
        assert_eq!(openrpc.methods.len(), 1);
        assert!(openrpc.servers.is_some());
        assert!(openrpc.components.is_some());
    }

    #[test]
    fn server_helpers_replace_append_and_expose_default_server() {
        let info = Info::new("Test API", "1.0.0");
        let production = Server::new("production", "https://api.example.com");
        let staging = Server::new("staging", "https://staging.example.com");

        let openrpc = OpenRpc::v1_3_2(info, vec![])
            .with_servers(vec![production.clone()])
            .with_server(staging.clone());

        let servers = openrpc.get_servers();
        assert_eq!(servers, vec![&production, &staging]);

        let empty_servers = OpenRpc::v1_3_2(Info::new("Test API", "1.0.0"), vec![]);
        assert!(empty_servers.get_servers().is_empty());

        let default_server = OpenRpc::get_default_server();
        assert_eq!(default_server.name, "default");
        assert_eq!(default_server.url, "localhost");
    }

    #[test]
    fn method_or_reference_from_conversions_validate_both_variants() {
        let method: MethodOrReference = Method::new("direct", vec![]).into();
        assert!(matches!(method, MethodOrReference::Method(_)));
        assert!(method.validate().is_ok());

        let reference: MethodOrReference = Reference::schema("SharedSchema").into();
        assert!(matches!(reference, MethodOrReference::Reference(_)));
        assert!(reference.validate().is_ok());

        let invalid_reference = MethodOrReference::Reference(Reference::new(""));
        let err = invalid_reference.validate().unwrap_err();
        assert!(err.to_string().contains("Reference string cannot be empty"));
    }

    #[test]
    fn validation_errors_include_paths_for_nested_openrpc_objects() {
        let invalid_info = OpenRpc::v1_3_2(Info::new("", "1.0.0"), vec![]);
        let err = invalid_info.validate().unwrap_err();
        assert!(err.to_string().contains("info"));

        let invalid_server = OpenRpc::v1_3_2(Info::new("Test API", "1.0.0"), vec![])
            .with_server(Server::new("", "https://api.example.com"));
        let err = invalid_server.validate().unwrap_err();
        assert!(err.to_string().contains("servers[0]"));

        let invalid_method = OpenRpc::v1_3_2(
            Info::new("Test API", "1.0.0"),
            vec![Method::new("bad method", vec![]).into()],
        );
        let err = invalid_method.validate().unwrap_err();
        assert!(err.to_string().contains("methods[0]"));

        let invalid_components = OpenRpc::v1_3_2(Info::new("Test API", "1.0.0"), vec![])
            .with_components(Components::new().with_schema("invalid key", Schema::string()));
        let err = invalid_components.validate().unwrap_err();
        assert!(err.to_string().contains("components"));

        let invalid_external_docs = OpenRpc::v1_3_2(Info::new("Test API", "1.0.0"), vec![])
            .with_external_docs(ExternalDocumentation::new("not-a-url"));
        let err = invalid_external_docs.validate().unwrap_err();
        assert!(err.to_string().contains("externalDocs"));
    }

    #[test]
    fn validation_rejects_duplicate_reference_method_keys() {
        let openrpc = OpenRpc::v1_3_2(
            Info::new("Test API", "1.0.0"),
            vec![
                MethodOrReference::Reference(Reference::schema("Shared")),
                MethodOrReference::Reference(Reference::schema("Shared")),
            ],
        );

        let err = openrpc.validate().unwrap_err();
        assert!(err.to_string().contains("Duplicate key"));
        assert!(err.to_string().contains("methods"));
    }

    #[test]
    fn validation_rejects_invalid_openrpc_extensions() {
        let invalid_extensions: Extensions =
            HashMap::from([("x-".to_string(), json!("missing suffix"))]).into();
        let openrpc = OpenRpc {
            extensions: invalid_extensions,
            ..OpenRpc::v1_3_2(Info::new("Test API", "1.0.0"), vec![])
        };

        let err = openrpc.validate().unwrap_err();
        assert!(err.to_string().contains("Extension key must have content"));
    }
}
