use super::*;

#[tokio::test]
async fn test_invalid_requests() {
    let server = create_test_server();

    // Test method not found
    let response = make_jsonrpc_request(&server, "non_existent_method", json!(()), None).await;

    assert!(response.get("error").is_some());
    let error = &response["error"];
    assert_eq!(error["code"], -32601); // Method not found

    // Test invalid JSON-RPC format (missing jsonrpc field)
    let invalid_request = json!({
        "method": "sign_in",
        "params": {},
        "id": 1
    });

    let json_response: Value = server.post("/rpc").json(&invalid_request).await.json();
    assert!(json_response.get("error").is_some());

    // Test invalid parameters for a method
    let response = make_jsonrpc_request(&server, "sign_in", json!("invalid_params"), None).await;

    assert!(response.get("error").is_some());
}
