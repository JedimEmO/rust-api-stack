#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    // Test types
    #[derive(Serialize, Deserialize, Debug)]
    struct TestRequest {
        value: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct TestResponse {
        result: String,
    }

    // Generate a test service using the macro
    ras_jsonrpc_macro::jsonrpc_service!({
        service_name: TestService,
        methods: [
            UNAUTHORIZED test_internal_error(TestRequest) -> TestResponse,
            WITH_PERMISSIONS(["admin"]) test_auth_error(TestRequest) -> TestResponse,
        ]
    });

    struct TestServiceImpl;

    impl TestServiceTrait for TestServiceImpl {
        async fn test_internal_error(
            &self,
            _request: TestRequest,
        ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
            // Simulate various internal errors that should be sanitized
            Err("Database connection failed: unable to connect to postgres://user:password@localhost:5432/mydb".into())
        }

        async fn test_auth_error(
            &self,
            _user: &ras_jsonrpc_core::AuthenticatedUser,
            _request: TestRequest,
        ) -> Result<TestResponse, Box<dyn std::error::Error + Send + Sync>> {
            Ok(TestResponse {
                result: "success".to_string(),
            })
        }
    }

    fn setup_test_server() -> axum_test::TestServer {
        let router = TestServiceBuilder::new(TestServiceImpl)
            .base_url("/api/rpc")
            .build()
            .expect("Failed to build router");

        axum_test::TestServer::builder()
            .mock_transport()
            .build(router)
            .unwrap()
    }

    async fn rpc(server: &axum_test::TestServer, body: Value) -> Value {
        server.post("/api/rpc").json(&body).await.json()
    }

    #[tokio::test]
    async fn test_internal_error_sanitization() {
        let server = setup_test_server();

        let json_response = rpc(
            &server,
            json!({
                "jsonrpc": "2.0",
                "method": "test_internal_error",
                "params": {
                    "value": "test"
                },
                "id": 1
            }),
        )
        .await;
        println!(
            "Response: {}",
            serde_json::to_string_pretty(&json_response).unwrap()
        );

        // Verify error is sanitized
        assert_eq!(json_response["jsonrpc"], "2.0");
        assert_eq!(json_response["id"], 1);
        assert!(json_response["error"].is_object());
        assert_eq!(json_response["error"]["code"], -32603); // Internal error code
        assert_eq!(json_response["error"]["message"], "Internal error"); // Sanitized message
    }

    #[tokio::test]
    async fn test_invalid_params_error_sanitization() {
        let server = setup_test_server();

        let json_response = rpc(
            &server,
            json!({
                "jsonrpc": "2.0",
                "method": "test_internal_error",
                "params": {
                    "wrong_field": "test" // Invalid field name
                },
                "id": 2
            }),
        )
        .await;

        // Verify error is sanitized
        assert_eq!(json_response["jsonrpc"], "2.0");
        assert_eq!(json_response["id"], 2);
        assert!(json_response["error"].is_object());
        assert_eq!(json_response["error"]["code"], -32602); // Invalid params code
        assert_eq!(json_response["error"]["message"], "Invalid params"); // Sanitized message
    }

    #[tokio::test]
    async fn test_authentication_error_sanitization() {
        let server = setup_test_server();

        // Test missing auth header
        let json_response = rpc(
            &server,
            json!({
                "jsonrpc": "2.0",
                "method": "test_auth_error",
                "params": {
                    "value": "test"
                },
                "id": 3
            }),
        )
        .await;

        // Verify error is sanitized
        assert_eq!(json_response["jsonrpc"], "2.0");
        assert_eq!(json_response["id"], 3);
        assert!(json_response["error"].is_object());
        assert_eq!(json_response["error"]["code"], -32001); // Authentication required code
        assert_eq!(json_response["error"]["message"], "Authentication required"); // Standard message
    }

    #[tokio::test]
    async fn test_method_not_found_includes_method_name() {
        let server = setup_test_server();

        let json_response = rpc(
            &server,
            json!({
                "jsonrpc": "2.0",
                "method": "non_existent_method",
                "params": {},
                "id": 4
            }),
        )
        .await;

        // Verify error includes method name (this is safe to expose)
        assert_eq!(json_response["jsonrpc"], "2.0");
        assert_eq!(json_response["id"], 4);
        assert!(json_response["error"].is_object());
        assert_eq!(json_response["error"]["code"], -32601); // Method not found code
        assert_eq!(
            json_response["error"]["message"],
            "Method not found: non_existent_method"
        );
    }

    #[tokio::test]
    async fn test_parse_error_sanitization() {
        let server = setup_test_server();
        let json_response: Value = server
            .post("/api/rpc")
            .text("invalid json {")
            .content_type("application/json")
            .await
            .json();

        // Verify error is sanitized
        assert_eq!(json_response["jsonrpc"], "2.0");
        assert!(json_response["id"].is_null());
        assert!(json_response["error"].is_object());
        assert_eq!(json_response["error"]["code"], -32700); // Parse error code
        assert_eq!(json_response["error"]["message"], "Parse error"); // Standard message
    }
}
