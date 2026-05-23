use std::collections::HashSet;

use anyhow::Result;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_rest_core::{RestResponse, RestResult};
use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Health status returned by the fixture service.
///
/// **Schema docs** should render for REST.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HealthResponse {
    /// Current health state.
    /// This field description keeps its line break.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Widget {
    pub id: String,
    pub name: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WidgetsResponse {
    pub widgets: Vec<Widget>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateWidgetRequest {
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

rest_service!({
    service_name: ExplorerRestFixture,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    endpoints: [
        /// Check fixture `health`.
        ///
        /// **REST docs** support Markdown.
        /// - Shows operation details
        /// - Preserves line breaks
        ///
        /// Alpha line
        /// Beta line
        ///
        /// ```json
        /// {"status":"ok"}
        /// ```
        ///
        /// See [REST docs](https://github.com/JedimEmO/rust-agent-stack/blob/main/documentation/ras-rest-macro.md).
        GET UNAUTHORIZED health() -> HealthResponse,
        GET UNAUTHORIZED widgets/{id: String}() -> Widget,
        GET UNAUTHORIZED search/widgets ? q: String & limit: Option<u32> () -> WidgetsResponse,
        POST UNAUTHORIZED v2/widgets/{id: String}/rename(RenameWidgetV2) -> Widget {
            version: v2,
            versions: [
                v1 {
                    path: v1/widgets/{id: String}/rename,
                    body: RenameWidgetV1,
                    response: RenameWidgetResponseV1,
                    migration: RenameWidgetCompat,
                },
            ],
        },
        POST WITH_PERMISSIONS(["admin"]) widgets(CreateWidgetRequest) -> Widget,
        GET WITH_PERMISSIONS(["user"]) profile() -> ProfileResponse,
    ]
});

struct RenameWidgetCompat;

impl
    ras_rest_core::VersionMigration<
        ExplorerRestFixturePostV2WidgetsByIdRenameV1Request,
        ExplorerRestFixturePostV2WidgetsByIdRenameV2Request,
    > for RenameWidgetCompat
{
    type Error = std::convert::Infallible;

    fn migrate(
        value: ExplorerRestFixturePostV2WidgetsByIdRenameV1Request,
    ) -> Result<ExplorerRestFixturePostV2WidgetsByIdRenameV2Request, Self::Error> {
        Ok(ExplorerRestFixturePostV2WidgetsByIdRenameV2Request {
            path: ExplorerRestFixturePostV2WidgetsByIdRenameV2Path { id: value.path.id },
            query: ExplorerRestFixturePostV2WidgetsByIdRenameV2Query {},
            body: RenameWidgetV2 {
                display_name: value.body.name,
                notify: false,
            },
        })
    }
}

impl ras_rest_core::VersionMigration<Widget, RenameWidgetResponseV1> for RenameWidgetCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: Widget) -> Result<RenameWidgetResponseV1, Self::Error> {
        Ok(RenameWidgetResponseV1 { name: value.name })
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

struct FixtureService;

#[async_trait::async_trait]
impl ExplorerRestFixtureTrait for FixtureService {
    async fn get_health(&self) -> RestResult<HealthResponse> {
        Ok(RestResponse::ok(HealthResponse {
            status: "ok".to_string(),
        }))
    }

    async fn get_widgets_by_id(&self, id: String) -> RestResult<Widget> {
        Ok(RestResponse::ok(Widget {
            id,
            name: "Fixture Widget".to_string(),
            owner: "public".to_string(),
        }))
    }

    async fn get_search_widgets(
        &self,
        q: String,
        limit: Option<u32>,
    ) -> RestResult<WidgetsResponse> {
        let count = limit.unwrap_or(2).min(5) as usize;
        let widgets = (0..count)
            .map(|idx| Widget {
                id: format!("widget-{idx}"),
                name: format!("{q}-{idx}"),
                owner: "search".to_string(),
            })
            .collect::<Vec<_>>();

        Ok(RestResponse::ok(WidgetsResponse {
            total: widgets.len(),
            widgets,
        }))
    }

    async fn post_v2_widgets_by_id_rename(
        &self,
        id: String,
        request: RenameWidgetV2,
    ) -> RestResult<Widget> {
        Ok(RestResponse::ok(Widget {
            id,
            name: request.display_name,
            owner: if request.notify { "notified" } else { "silent" }.to_string(),
        }))
    }

    async fn post_widgets(
        &self,
        _user: &AuthenticatedUser,
        request: CreateWidgetRequest,
    ) -> RestResult<Widget> {
        Ok(RestResponse::created(Widget {
            id: "created-widget".to_string(),
            name: request.name,
            owner: request.owner,
        }))
    }

    async fn get_profile(&self, user: &AuthenticatedUser) -> RestResult<ProfileResponse> {
        Ok(RestResponse::ok(ProfileResponse {
            user_id: user.user_id.clone(),
            permissions: user.permissions.iter().cloned().collect(),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = ExplorerRestFixtureBuilder::new(FixtureService)
        .auth_provider(FixtureAuthProvider)
        .build();

    let bind_addr =
        std::env::var("PLAYWRIGHT_REST_ADDR").unwrap_or_else(|_| "127.0.0.1:3101".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use serde_json::{Value, json};

    fn test_server() -> TestServer {
        let app = ExplorerRestFixtureBuilder::new(FixtureService)
            .auth_provider(FixtureAuthProvider)
            .build();

        TestServer::builder()
            .mock_transport()
            .build(app)
            .expect("in-memory axum-test server")
    }

    fn parameter<'a>(operation: &'a Value, name: &str) -> &'a Value {
        operation["parameters"]
            .as_array()
            .expect("parameters array")
            .iter()
            .find(|parameter| parameter["name"] == name)
            .unwrap_or_else(|| panic!("missing parameter {name}"))
    }

    #[test]
    fn generated_openapi_documents_explorer_fixture_routes() {
        let doc = generate_explorerrestfixture_openapi();

        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "ExplorerRestFixture REST API");

        let health = &doc["paths"]["/health"]["get"];
        assert_eq!(health["summary"], json!("Check fixture `health`."));
        assert!(
            health["description"]
                .as_str()
                .expect("health description")
                .contains("Preserves line breaks")
        );

        assert!(doc["paths"]["/widgets/{id}"]["get"].is_object());
        assert!(doc["paths"]["/search/widgets"]["get"].is_object());
        assert!(doc["paths"]["/v1/widgets/{id}/rename"]["post"].is_object());
        assert!(doc["paths"]["/v2/widgets/{id}/rename"]["post"].is_object());
    }

    #[test]
    fn generated_openapi_keeps_auth_query_and_version_metadata() {
        let doc = generate_explorerrestfixture_openapi();

        let search_widgets = &doc["paths"]["/search/widgets"]["get"];
        assert_eq!(parameter(search_widgets, "q")["required"], json!(true));
        assert_eq!(parameter(search_widgets, "limit")["required"], json!(false));

        let create_widget = &doc["paths"]["/widgets"]["post"];
        assert_eq!(create_widget["security"][0]["bearerAuth"], json!([]));
        assert_eq!(create_widget["x-permissions"], json!(["admin"]));

        let rename_v1 = &doc["paths"]["/v1/widgets/{id}/rename"]["post"];
        assert_eq!(rename_v1["x-ras-version"], json!("v1"));
        assert_eq!(rename_v1["x-ras-canonical-version"], json!("v2"));
        assert_eq!(
            rename_v1["x-ras-canonical-path"],
            json!("/v2/widgets/{id}/rename")
        );

        let rename_v2 = &doc["paths"]["/v2/widgets/{id}/rename"]["post"];
        assert_eq!(rename_v2["x-ras-version"], json!("v2"));
        assert_eq!(parameter(rename_v2, "id")["required"], json!(true));
    }

    #[test]
    fn rename_widget_compat_upgrades_request_and_downgrades_response() {
        let upgraded = <RenameWidgetCompat as ras_rest_core::VersionMigration<
            ExplorerRestFixturePostV2WidgetsByIdRenameV1Request,
            ExplorerRestFixturePostV2WidgetsByIdRenameV2Request,
        >>::migrate(ExplorerRestFixturePostV2WidgetsByIdRenameV1Request {
            path: ExplorerRestFixturePostV2WidgetsByIdRenameV1Path {
                id: "widget-1".to_string(),
            },
            query: ExplorerRestFixturePostV2WidgetsByIdRenameV1Query {},
            body: RenameWidgetV1 {
                name: "legacy name".to_string(),
            },
        })
        .expect("legacy request migrates");

        assert_eq!(upgraded.path.id, "widget-1");
        assert_eq!(upgraded.body.display_name, "legacy name");
        assert!(!upgraded.body.notify);

        let downgraded = <RenameWidgetCompat as ras_rest_core::VersionMigration<
            Widget,
            RenameWidgetResponseV1,
        >>::migrate(Widget {
            id: "widget-1".to_string(),
            name: "canonical name".to_string(),
            owner: "fixture".to_string(),
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
    async fn generated_rest_routes_round_trip_without_socket() {
        let server = test_server();

        let health = server.get("/api/v1/health").await;
        health.assert_status_ok();
        let health: HealthResponse = health.json();
        assert_eq!(health.status, "ok");

        let search = server.get("/api/v1/search/widgets?q=docs&limit=3").await;
        search.assert_status_ok();
        let search: WidgetsResponse = search.json();
        assert_eq!(search.total, 3);
        assert_eq!(search.widgets[0].name, "docs-0");
        assert_eq!(search.widgets[2].id, "widget-2");
    }

    #[tokio::test]
    async fn generated_rest_routes_enforce_permissions_without_socket() {
        let server = test_server();

        let user_response = server
            .post("/api/v1/widgets")
            .authorization_bearer("user-token")
            .json(&json!({
                "name": "Fixture Widget",
                "owner": "docs",
            }))
            .await;
        user_response.assert_status(StatusCode::FORBIDDEN);

        let admin_response = server
            .post("/api/v1/widgets")
            .authorization_bearer("admin-token")
            .json(&json!({
                "name": "Fixture Widget",
                "owner": "docs",
            }))
            .await;
        admin_response.assert_status(StatusCode::CREATED);
        let widget: Widget = admin_response.json();
        assert_eq!(widget.id, "created-widget");
        assert_eq!(widget.owner, "docs");
    }

    #[tokio::test]
    async fn generated_docs_routes_serve_explorer_and_openapi_without_socket() {
        let server = test_server();

        let docs = server.get("/api/v1/docs").await;
        docs.assert_status_ok();
        assert!(docs.text().contains("ExplorerRestFixture"));

        let spec = server.get("/api/v1/docs/openapi.json").await;
        spec.assert_status_ok();
        let doc: Value = spec.json();
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["info"]["title"], "ExplorerRestFixture REST API");
    }
}
