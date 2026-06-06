#[cfg(all(feature = "server", feature = "client"))]
mod tests {
    use axum::Router;
    use ras_jsonrpc_macro::jsonrpc_service;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct CreateUserRequest {
        username: String,
        email: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct CreateUserResponse {
        user_id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct GetUserRequest {
        user_id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct GetUserResponse {
        user_id: String,
        username: String,
        email: String,
    }

    jsonrpc_service!({
        service_name: UserService,
        openrpc: true,
        explorer: true,
        methods: [
            UNAUTHORIZED create_user(CreateUserRequest) -> CreateUserResponse,
            WITH_PERMISSIONS(["admin"]) get_user(GetUserRequest) -> GetUserResponse,
        ]
    });

    #[tokio::test]
    async fn test_explorer_routes_generated() {
        // Test that the explorer routes function is generated
        let explorer_routes = userservice_explorer_routes("");

        // The router should have routes for /explorer and /explorer/openrpc.json
        let app = Router::new().merge(explorer_routes);
        let server = axum_test::TestServer::builder()
            .mock_transport()
            .build(app)
            .unwrap();

        // Test that the explorer page is accessible
        let response = server.get("/explorer").await;
        response.assert_status_ok();

        let content = response.text();
        assert!(content.contains("\"UserService\""));
        assert!(content.contains("id=\"bearer-token\""));
        assert!(content.contains("id=\"saved-list\""));
        assert!(content.contains("id=\"history-list\""));
        assert!(content.contains("\"jsonrpc\""));

        // Test that the OpenRPC document is accessible
        let response = server.get("/explorer/openrpc.json").await;
        response.assert_status_ok();

        let openrpc_doc: serde_json::Value = response.json();
        assert_eq!(openrpc_doc["info"]["title"], "UserService JSON-RPC API");
        assert!(openrpc_doc["methods"].is_array());
    }

    #[test]
    fn test_explorer_with_custom_path() {
        mod custom_path_service {
            use ras_jsonrpc_macro::jsonrpc_service;

            jsonrpc_service!({
                service_name: TestService,
                openrpc: true,
                explorer: { path: "/api/docs" },
                methods: [
                    UNAUTHORIZED test_method(()) -> String,
                ]
            });

            pub fn test_routes() {
                // Test that the explorer routes function is generated
                let _explorer_routes = testservice_explorer_routes("/api/docs");
            }
        }

        custom_path_service::test_routes();
    }

    #[tokio::test]
    async fn explorer_custom_path_is_normalized_and_nested_under_base_url() {
        mod normalized_path_service {
            use ras_jsonrpc_macro::jsonrpc_service;
            use serde::{Deserialize, Serialize};

            #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
            pub struct PingRequest;

            #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
            pub struct PingResponse;

            jsonrpc_service!({
                service_name: NormalizedService,
                openrpc: true,
                explorer: { path: "api/docs/" },
                methods: [
                    UNAUTHORIZED ping(PingRequest) -> PingResponse,
                ]
            });

            pub fn routes(base_url: &str) -> axum::Router {
                normalizedservice_explorer_routes(base_url)
            }
        }

        let app = normalized_path_service::routes("/rpc/");
        let server = axum_test::TestServer::builder()
            .mock_transport()
            .build(app)
            .unwrap();

        let docs_response = server.get("/rpc/api/docs").await;
        docs_response.assert_status_ok();
        assert!(docs_response.text().contains("\"NormalizedService\""));

        let spec_response = server.get("/rpc/api/docs/openrpc.json").await;
        spec_response.assert_status_ok();
        let spec: serde_json::Value = spec_response.json();
        assert_eq!(spec["info"]["title"], "NormalizedService JSON-RPC API");
    }

    #[test]
    fn test_explorer_requires_openrpc() {
        mod no_openrpc_service {
            use ras_jsonrpc_macro::jsonrpc_service;

            jsonrpc_service!({
                service_name: NoOpenRpcService,
                explorer: true,  // This should be ignored without openrpc
                methods: [
                    UNAUTHORIZED test_method(()) -> String,
                ]
            });
        }

        // Explorer routes should not be generated
        // This test just verifies that the macro compiles
    }
}
