use std::collections::HashSet;

use anyhow::Result;
use axum::Router;
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Request payload for the `ping` method.
///
/// **Schema docs** should render with Markdown.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PingRequest {
    /// Message echoed by the fixture service.
    /// This line must stay on a new line.
    pub message: String,
}

/// Response returned by the ping method.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PingResponse {
    /// Message returned from the fixture service.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateWidgetRequest {
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Widget {
    pub id: String,
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileResponse {
    pub user_id: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetV1 {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetV2 {
    pub display_name: String,
    pub notify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetResponseV1 {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetResponseV2 {
    pub display_name: String,
    pub notified: bool,
}

jsonrpc_service!({
    service_name: ExplorerRpcFixture,
    openrpc: true,
    explorer: true,
    methods: [
        /// Echo a `PingRequest` message.
        ///
        /// **Use this in tests.**
        /// - Confirms list rendering
        /// - Preserves list items
        ///
        /// Line one
        /// Line two
        ///
        /// ```json
        /// {"message":"hello"}
        /// ```
        ///
        /// See [Rust API Stack](https://github.com/JedimEmO/rust-api-stack/blob/main/crates/rpc/ras-jsonrpc-macro/README.md).
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
        UNAUTHORIZED no_params(()) -> String,
        UNAUTHORIZED rename_widget(RenameWidgetV2) -> RenameWidgetResponseV2 {
            version: v2,
            wire: "rename_widget.v2",
            versions: [
                v1 {
                    wire: "rename_widget.v1",
                    request: RenameWidgetV1,
                    response: RenameWidgetResponseV1,
                    migration: RenameWidgetCompat,
                },
            ],
        },
        WITH_PERMISSIONS(["admin"]) create_widget(CreateWidgetRequest) -> Widget,
        WITH_PERMISSIONS(["user"]) current_profile(()) -> ProfileResponse,
    ]
});

struct RenameWidgetCompat;

impl ras_jsonrpc_core::VersionMigration<RenameWidgetV1, RenameWidgetV2> for RenameWidgetCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameWidgetV1) -> Result<RenameWidgetV2, Self::Error> {
        Ok(RenameWidgetV2 {
            display_name: value.name,
            notify: false,
        })
    }
}

impl ras_jsonrpc_core::VersionMigration<RenameWidgetResponseV2, RenameWidgetResponseV1>
    for RenameWidgetCompat
{
    type Error = std::convert::Infallible;

    fn migrate(value: RenameWidgetResponseV2) -> Result<RenameWidgetResponseV1, Self::Error> {
        Ok(RenameWidgetResponseV1 {
            name: value.display_name,
        })
    }
}

struct FixtureAuthProvider;

impl AuthProvider for FixtureAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let (user_id, permissions) = match token.as_str() {
                "user-token" => ("user-1", vec!["user"]),
                "admin-token" => ("admin-1", vec!["user", "admin"]),
                _ => return Err(AuthError::InvalidToken),
            };

            Ok(AuthenticatedUser {
                user_id: user_id.to_string(),
                permissions: permissions
                    .into_iter()
                    .map(str::to_string)
                    .collect::<HashSet<_>>(),
                metadata: None,
            })
        })
    }
}

struct ExplorerRpcFixtureImpl;

impl ExplorerRpcFixtureTrait for ExplorerRpcFixtureImpl {
    async fn ping(
        &self,
        request: PingRequest,
    ) -> std::result::Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            message: format!("pong: {}", request.message),
        })
    }

    async fn no_params(
        &self,
        _request: (),
    ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("no params ok".to_string())
    }

    async fn rename_widget(
        &self,
        request: RenameWidgetV2,
    ) -> std::result::Result<RenameWidgetResponseV2, Box<dyn std::error::Error + Send + Sync>> {
        Ok(RenameWidgetResponseV2 {
            display_name: request.display_name,
            notified: request.notify,
        })
    }

    async fn create_widget(
        &self,
        _user: &AuthenticatedUser,
        request: CreateWidgetRequest,
    ) -> std::result::Result<Widget, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Widget {
            id: "rpc-created-widget".to_string(),
            name: request.name,
            owner: request.owner,
        })
    }

    async fn current_profile(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> std::result::Result<ProfileResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(ProfileResponse {
            user_id: user.user_id.clone(),
            permissions: user.permissions.iter().cloned().collect(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let rpc_router = ExplorerRpcFixtureBuilder::new(ExplorerRpcFixtureImpl)
        .base_url("/rpc")
        .auth_provider(FixtureAuthProvider)
        .build()
        .expect("fixture JSON-RPC service should build");

    let app = Router::new().merge(rpc_router);
    let bind_addr =
        std::env::var("PLAYWRIGHT_JSONRPC_ADDR").unwrap_or_else(|_| "127.0.0.1:3102".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use serde_json::json;
    use std::collections::BTreeSet;

    fn test_server() -> TestServer {
        let rpc_router = ExplorerRpcFixtureBuilder::new(ExplorerRpcFixtureImpl)
            .base_url("/rpc")
            .auth_provider(FixtureAuthProvider)
            .build()
            .expect("fixture JSON-RPC service should build");

        TestServer::builder()
            .mock_transport()
            .build(Router::new().merge(rpc_router))
            .expect("in-memory axum-test server")
    }

    async fn jsonrpc_request(
        server: &TestServer,
        method: &str,
        params: serde_json::Value,
        token: Option<&str>,
    ) -> serde_json::Value {
        let body = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });
        let mut request = server.post("/rpc").json(&body);

        if let Some(token) = token {
            request = request.authorization_bearer(token);
        }

        request.await.json()
    }

    fn method<'a>(doc: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        doc["methods"]
            .as_array()
            .expect("methods array")
            .iter()
            .find(|method| method["name"] == name)
            .unwrap_or_else(|| panic!("missing method {name}"))
    }

    #[test]
    fn generated_openrpc_documents_explorer_fixture_methods() {
        let doc = generate_explorerrpcfixture_openrpc();

        assert_eq!(doc["openrpc"], "1.3.2");
        assert_eq!(doc["info"]["title"], "ExplorerRpcFixture JSON-RPC API");

        let method_names = doc["methods"]
            .as_array()
            .expect("methods array")
            .iter()
            .map(|method| method["name"].as_str().expect("method name"))
            .collect::<BTreeSet<_>>();

        assert_eq!(
            method_names,
            BTreeSet::from([
                "create_widget",
                "current_profile",
                "no_params",
                "ping",
                "rename_widget.v1",
                "rename_widget.v2",
            ])
        );
    }

    #[test]
    fn generated_openrpc_keeps_docs_permissions_and_version_metadata() {
        let doc = generate_explorerrpcfixture_openrpc();

        let ping = method(&doc, "ping");
        assert_eq!(ping["summary"], json!("Echo a `PingRequest` message."));
        assert!(
            ping["description"]
                .as_str()
                .expect("ping description")
                .contains("Preserves list items")
        );
        assert!(ping.get("x-authentication").is_none());

        let create_widget = method(&doc, "create_widget");
        assert_eq!(
            create_widget["x-authentication"]["required"].as_bool(),
            Some(true)
        );
        assert_eq!(create_widget["x-permissions"], json!(["admin"]));

        let rename_v1 = method(&doc, "rename_widget.v1");
        assert_eq!(rename_v1["x-ras-version"], json!("v1"));
        assert_eq!(rename_v1["x-ras-canonical-version"], json!("v2"));
        assert_eq!(
            rename_v1["x-ras-canonical-method"],
            json!("rename_widget.v2")
        );

        let rename_v2 = method(&doc, "rename_widget.v2");
        assert_eq!(rename_v2["x-ras-version"], json!("v2"));
    }

    #[test]
    fn rename_widget_compat_upgrades_request_and_downgrades_response() {
        let upgraded = <RenameWidgetCompat as ras_jsonrpc_core::VersionMigration<
            RenameWidgetV1,
            RenameWidgetV2,
        >>::migrate(RenameWidgetV1 {
            name: "legacy name".to_string(),
        })
        .expect("legacy request migrates");

        assert_eq!(upgraded.display_name, "legacy name");
        assert!(!upgraded.notify);

        let downgraded = <RenameWidgetCompat as ras_jsonrpc_core::VersionMigration<
            RenameWidgetResponseV2,
            RenameWidgetResponseV1,
        >>::migrate(RenameWidgetResponseV2 {
            display_name: "canonical name".to_string(),
            notified: true,
        })
        .expect("canonical response migrates");

        assert_eq!(downgraded.name, "canonical name");
    }

    #[tokio::test]
    async fn fixture_auth_provider_maps_tokens_to_permission_sets() {
        let user = FixtureAuthProvider
            .authenticate("user-token".to_string())
            .await
            .expect("user token authenticates");
        assert_eq!(user.user_id, "user-1");
        assert!(user.permissions.contains("user"));
        assert!(!user.permissions.contains("admin"));

        let admin = FixtureAuthProvider
            .authenticate("admin-token".to_string())
            .await
            .expect("admin token authenticates");
        assert_eq!(admin.user_id, "admin-1");
        assert!(admin.permissions.contains("user"));
        assert!(admin.permissions.contains("admin"));

        assert!(
            FixtureAuthProvider
                .authenticate("invalid-token".to_string())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn generated_jsonrpc_routes_round_trip_without_socket() {
        let server = test_server();

        let response = jsonrpc_request(&server, "ping", json!({ "message": "hello" }), None).await;

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["message"], "pong: hello");
        assert!(response.get("error").is_none());

        let response = jsonrpc_request(&server, "no_params", json!(null), None).await;
        assert_eq!(response["result"], "no params ok");
    }

    #[tokio::test]
    async fn generated_jsonrpc_routes_enforce_permissions_without_socket() {
        let server = test_server();
        let params = json!({
            "name": "Fixture Widget",
            "owner": "docs",
        });

        let user_response =
            jsonrpc_request(&server, "create_widget", params.clone(), Some("user-token")).await;
        assert!(user_response.get("result").is_none());
        assert!(user_response.get("error").is_some());

        let admin_response =
            jsonrpc_request(&server, "create_widget", params, Some("admin-token")).await;
        assert_eq!(admin_response["result"]["id"], "rpc-created-widget");
        assert_eq!(admin_response["result"]["owner"], "docs");
        assert!(admin_response.get("error").is_none());
    }

    #[tokio::test]
    async fn generated_openrpc_route_serves_document_without_socket() {
        let server = test_server();

        let response = server.get("/rpc/explorer/openrpc.json").await;

        response.assert_status_ok();
        let doc: serde_json::Value = response.json();
        assert_eq!(doc["openrpc"], "1.3.2");
        assert_eq!(doc["info"]["title"], "ExplorerRpcFixture JSON-RPC API");
    }
}
