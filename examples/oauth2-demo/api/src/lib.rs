use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request to get current user information
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetUserInfoRequest {}

/// Response containing user information
#[derive(Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct GetUserInfoResponse {
    pub user_id: String,
    pub permissions: Vec<String>,
    #[schemars(schema_with = "optional_object_schema")]
    pub metadata: Option<serde_json::Value>,
}

/// Schema function that returns an optional object schema
fn optional_object_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), serde_json::json!("object"));
    schemars::Schema::from(map)
}

/// Request to create a new document (admin only)
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CreateDocumentRequest {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
}

/// Response for document creation
#[derive(Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct CreateDocumentResponse {
    pub document_id: String,
    pub created_at: String,
}

/// Request to list documents
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ListDocumentsRequest {
    #[schemars(flatten)]
    pub limit: Option<u32>,
    #[schemars(flatten)]
    pub offset: Option<u32>,
}

/// Document information
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocumentInfo {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub tags: Vec<String>,
}

/// Response for listing documents
#[derive(Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct ListDocumentsResponse {
    pub documents: Vec<DocumentInfo>,
    pub total: u32,
}

/// Request to delete a document (admin only)
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DeleteDocumentRequest {
    pub document_id: String,
}

/// Response for document deletion
#[derive(Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct DeleteDocumentResponse {
    pub success: bool,
    pub message: String,
}

/// Request to get system status (system admin only)
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetSystemStatusRequest {}

/// System status information
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SystemStatus {
    pub uptime_seconds: u64,
    pub memory_usage_mb: u64,
    pub active_sessions: u32,
    pub version: String,
}

/// Response for system status
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetSystemStatusResponse {
    pub status: SystemStatus,
}

impl Default for GetSystemStatusResponse {
    fn default() -> Self {
        Self {
            status: SystemStatus {
                uptime_seconds: 0,
                memory_usage_mb: 0,
                active_sessions: 0,
                version: "1.0.0".to_string(),
            },
        }
    }
}

/// Request to access beta features
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetBetaFeaturesRequest {}

/// Beta feature information
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct BetaFeature {
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

/// Response for beta features
#[derive(Debug, Serialize, Deserialize, Default, JsonSchema)]
pub struct GetBetaFeaturesResponse {
    pub features: Vec<BetaFeature>,
}

// Define the JSON-RPC service using the macro
jsonrpc_service!({
    service_name: GoogleOAuth2Service,
    openrpc: true,
    feature_gated: true,
    methods: [
        // Public endpoints (require authentication but no specific permissions)
        WITH_PERMISSIONS([]) get_user_info(GetUserInfoRequest) -> GetUserInfoResponse,
        WITH_PERMISSIONS(["user:read"]) list_documents(ListDocumentsRequest) -> ListDocumentsResponse,

        // Content creation and editing (requires elevated permissions)
        WITH_PERMISSIONS(["content:create"]) create_document(CreateDocumentRequest) -> CreateDocumentResponse,

        // Admin operations
        WITH_PERMISSIONS(["admin:write"]) delete_document(DeleteDocumentRequest) -> DeleteDocumentResponse,

        // System administration
        WITH_PERMISSIONS(["system:admin"]) get_system_status(GetSystemStatusRequest) -> GetSystemStatusResponse,

        // Beta features
        WITH_PERMISSIONS(["beta:access"]) get_beta_features(GetBetaFeaturesRequest) -> GetBetaFeaturesResponse,
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn list_documents_request_serializes_flat_optional_pagination() {
        let request = ListDocumentsRequest {
            limit: Some(25),
            offset: None,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({ "limit": 25, "offset": null })
        );
    }

    #[test]
    fn default_system_status_response_uses_demo_version() {
        let response = GetSystemStatusResponse::default();

        assert_eq!(response.status.uptime_seconds, 0);
        assert_eq!(response.status.memory_usage_mb, 0);
        assert_eq!(response.status.active_sessions, 0);
        assert_eq!(response.status.version, "1.0.0");
    }

    #[test]
    fn create_document_request_serializes_content_and_tags() {
        let request = CreateDocumentRequest {
            title: "Roadmap".to_string(),
            content: "Launch the documented demo".to_string(),
            tags: vec!["docs".to_string(), "demo".to_string()],
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "title": "Roadmap",
                "content": "Launch the documented demo",
                "tags": ["docs", "demo"]
            })
        );
    }

    #[test]
    fn beta_features_response_serializes_enabled_flags() {
        let response = GetBetaFeaturesResponse {
            features: vec![
                BetaFeature {
                    name: "workspace-search".to_string(),
                    description: "Search across workspace documents".to_string(),
                    enabled: true,
                },
                BetaFeature {
                    name: "admin-dashboard".to_string(),
                    description: "Preview admin dashboard".to_string(),
                    enabled: false,
                },
            ],
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "features": [
                    {
                        "name": "workspace-search",
                        "description": "Search across workspace documents",
                        "enabled": true
                    },
                    {
                        "name": "admin-dashboard",
                        "description": "Preview admin dashboard",
                        "enabled": false
                    }
                ]
            })
        );
    }

    #[test]
    fn user_info_response_serializes_permissions_and_metadata() {
        let response = GetUserInfoResponse {
            user_id: "user-1".to_string(),
            permissions: vec!["user:read".to_string(), "beta:access".to_string()],
            metadata: Some(json!({
                "email": "alice@example.test",
                "verified": true
            })),
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "user_id": "user-1",
                "permissions": ["user:read", "beta:access"],
                "metadata": {
                    "email": "alice@example.test",
                    "verified": true
                }
            })
        );
    }

    #[test]
    fn list_documents_response_serializes_documents_and_total() {
        let response = ListDocumentsResponse {
            documents: vec![DocumentInfo {
                id: "doc-1".to_string(),
                title: "Roadmap".to_string(),
                created_at: "2026-05-23T12:00:00Z".to_string(),
                tags: vec!["docs".to_string(), "demo".to_string()],
            }],
            total: 1,
        };

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "documents": [{
                    "id": "doc-1",
                    "title": "Roadmap",
                    "created_at": "2026-05-23T12:00:00Z",
                    "tags": ["docs", "demo"]
                }],
                "total": 1
            })
        );
    }

    #[test]
    fn delete_document_response_default_is_unsuccessful_empty_message() {
        let response = DeleteDocumentResponse::default();

        assert!(!response.success);
        assert!(response.message.is_empty());
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "success": false,
                "message": ""
            })
        );
    }

    #[test]
    fn generated_openrpc_documents_permissions_for_all_methods() {
        let doc = generate_googleoauth2service_openrpc();
        let methods = doc["methods"].as_array().expect("methods array");

        assert_eq!(doc["openrpc"], "1.3.2");
        assert_eq!(doc["info"]["title"], "GoogleOAuth2Service JSON-RPC API");

        let permissions_by_method = methods
            .iter()
            .map(|method| {
                let name = method["name"].as_str().expect("method name").to_string();
                let permissions = method
                    .get("x-permissions")
                    .and_then(|permissions| permissions.as_array())
                    .map(|permissions| {
                        permissions
                            .iter()
                            .map(|permission| permission.as_str().expect("permission").to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (name, permissions)
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            permissions_by_method,
            BTreeMap::from([
                (
                    "create_document".to_string(),
                    vec!["content:create".to_string()]
                ),
                (
                    "delete_document".to_string(),
                    vec!["admin:write".to_string()]
                ),
                (
                    "get_beta_features".to_string(),
                    vec!["beta:access".to_string()]
                ),
                (
                    "get_system_status".to_string(),
                    vec!["system:admin".to_string()]
                ),
                ("get_user_info".to_string(), vec![]),
                ("list_documents".to_string(), vec!["user:read".to_string()]),
            ])
        );

        let list_documents = methods
            .iter()
            .find(|method| method["name"] == "list_documents")
            .expect("list_documents method");
        assert_eq!(
            list_documents["x-permission-groups"],
            json!([["user:read"]])
        );
    }

    #[test]
    fn generated_openrpc_uses_object_schema_for_user_metadata() {
        let doc = generate_googleoauth2service_openrpc();
        let metadata =
            &doc["components"]["schemas"]["GetUserInfoResponse"]["properties"]["metadata"];

        assert_eq!(metadata["type"], json!("object"));
    }
}

#[cfg(test)]
mod permission_manifest_tests {
    use super::*;
    use ras_permission_manifest::{
        AuthRequirementInfo, OperationKind, PermissionSet, TransportKind, WireTarget,
    };

    #[test]
    fn generated_permission_manifest_distinguishes_authenticated_only_jsonrpc() {
        let manifest = generate_googleoauth2service_permission_manifest();

        assert_eq!(manifest.service_name, "GoogleOAuth2Service");
        assert_eq!(manifest.transport, TransportKind::JsonRpc);

        let user_info = manifest
            .operations
            .iter()
            .find(|operation| {
                matches!(
                    &operation.wire,
                    WireTarget::JsonRpc { method } if method == "get_user_info"
                )
            })
            .expect("get_user_info operation");

        assert_eq!(user_info.kind, OperationKind::JsonRpcMethod);
        assert_eq!(user_info.auth, AuthRequirementInfo::Authenticated);
    }

    #[test]
    fn generated_permission_constants_can_feed_token_permissions() {
        let permissions = PermissionSet::new()
            .with(googleoauth2service_permissions::USER_READ)
            .into_hash_set();

        assert!(permissions.contains("user:read"));
        assert!(
            googleoauth2service_permissions::operations::LIST_DOCUMENTS
                .is_satisfied_by(&permissions)
        );
    }
}
