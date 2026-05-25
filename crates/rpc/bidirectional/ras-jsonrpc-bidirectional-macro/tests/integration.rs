//! Integration tests for the bidirectional macro

use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRequest {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResponse {
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationData {
    pub message: String,
}

// Test the macro expansion
jsonrpc_bidirectional_service!({
    service_name: TestService,
    client_to_server: [
        UNAUTHORIZED test_method(TestRequest) -> TestResponse,
        WITH_PERMISSIONS(["admin"]) admin_method(String) -> bool,
    ],
    server_to_client: [
        user_notification(NotificationData),
        status_update(String),
    ],

    server_to_client_calls: [
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generated_code_compiles() {
        let request = TestRequest {
            data: "input".to_string(),
        };
        assert_eq!(request.data, "input");
    }
}
