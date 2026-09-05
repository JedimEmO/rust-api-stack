use super::*;

#[tokio::test]
async fn test_concurrent_rest_requests() {
    let server = Arc::new(create_rest_test_server());

    // Test multiple concurrent requests
    let mut handles = vec![];

    for _ in 0..10 {
        let server = Arc::clone(&server);
        let handle = tokio::spawn(async move {
            make_rest_request(&server, Method::GET, "/api/v1/health", None, None).await
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let results = futures::future::join_all(handles).await;

    // All requests should succeed
    for result in results {
        let response = result.unwrap();
        assert_eq!(response.status_code().as_u16(), 200);
        let health: String = response.json();
        assert_eq!(health, "OK");
    }
}

#[tokio::test]
async fn test_generated_rest_client() {
    // Real end-to-end test: drive the generated client over the in-process
    // AxumTestTransport against the live router. Covers unauthenticated GET,
    // query-param serialization, bearer auth, a unit-type response, and HTTP
    // error -> TransportError::Status mapping.
    let server = create_rest_test_server_arc();
    let mut client = create_rest_test_client(server);

    // Bearer-token accessors still behave as before.
    assert_eq!(client.bearer_token(), None);

    // 1. Unauthenticated GET returning a deserialized body.
    let users = client.get_users().await.expect("get_users failed");
    assert_eq!(users.total, 2);
    assert_eq!(users.users[0].name, "John Doe");

    // 2. Query params (required + optional) over the serde_urlencoded path.
    let search = client
        .get_search_users("john".to_string(), Some(5), Some(10))
        .await
        .expect("get_search_users failed");
    assert!(search.users[0].name.contains("john"));
    assert!(search.users[0].name.contains("offset 10"));

    // Optional query params omitted when None.
    let search = client
        .get_search_users("jane".to_string(), None, None)
        .await
        .expect("get_search_users without optionals failed");
    assert!(search.users[0].name.contains("jane"));

    // 3. Bearer auth: a permissioned GET succeeds once the token is set.
    client.set_bearer_token(Some("user-token"));
    assert_eq!(client.bearer_token(), Some("user-token"));
    let user = client
        .get_users_by_id(7)
        .await
        .expect("get_users_by_id with user token failed");
    assert_eq!(user.id, Some(7));

    // 4. Unit-type response (DELETE -> ()) with admin auth.
    let mut admin_client = create_rest_test_client(create_rest_test_server_arc());
    admin_client.set_bearer_token(Some("admin-token"));
    admin_client
        .delete_users_by_id(5)
        .await
        .expect("delete_users_by_id with admin token failed");

    // 5. HTTP error mapping: 404 -> TransportError::Status.
    let err = client
        .get_users_by_id(404)
        .await
        .expect_err("get_users_by_id(404) should fail");
    match err {
        ras_transport_core::TransportError::Status { status, .. } => {
            assert_eq!(status, ras_transport_core::http::StatusCode::NOT_FOUND);
        }
        other => panic!("expected TransportError::Status, got {other:?}"),
    }
}
