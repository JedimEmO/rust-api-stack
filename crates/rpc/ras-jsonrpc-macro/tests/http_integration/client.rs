use super::*;

#[tokio::test]
async fn test_concurrent_requests() {
    let server = std::sync::Arc::new(create_test_server());

    // Test multiple concurrent requests
    let mut handles = vec![];

    for _ in 0..10 {
        let server = std::sync::Arc::clone(&server);
        let handle = tokio::spawn(async move {
            make_jsonrpc_request(&server, "get_public_info", json!(()), None).await
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let results = futures::future::join_all(handles).await;

    // All requests should succeed
    for result in results {
        let response = result.unwrap();
        assert_eq!(response["result"], "This is public information");
    }
}

#[cfg(feature = "reqwest")]
#[test]
fn test_client_generation() {
    // Test that client generation compiles and produces valid API
    let client_result = TestServiceClientBuilder::new("http://example.invalid/rpc")
        .with_timeout(std::time::Duration::from_millis(1000))
        .build();

    assert!(client_result.is_ok());

    let mut client = client_result.unwrap();
    client.set_bearer_token(Some("test-token"));
    assert_eq!(client.bearer_token(), Some("test-token"));
}
