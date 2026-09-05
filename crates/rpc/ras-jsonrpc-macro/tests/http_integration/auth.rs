use super::*;

#[tokio::test]
async fn test_unauthorized_methods() {
    let server = create_test_server();

    // Test sign_in with valid credentials
    let response = make_jsonrpc_request(
        &server,
        "sign_in",
        json!({
            "email": "admin@test.com",
            "password": "admin123"
        }),
        None,
    )
    .await;

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response.get("error").is_none());

    let result = &response["result"];
    assert_eq!(result["jwt"], "valid-admin-token");
    assert_eq!(result["user_id"], "admin-user");

    // Test sign_in with invalid credentials
    let response = make_jsonrpc_request(
        &server,
        "sign_in",
        json!({
            "email": "wrong@test.com",
            "password": "wrong"
        }),
        None,
    )
    .await;

    assert!(response.get("error").is_some());

    // Test get_public_info
    let response = make_jsonrpc_request(&server, "get_public_info", json!(()), None).await;

    assert_eq!(response["result"], "This is public information");

    let request_body = json!({
        "jsonrpc": "2.0",
        "method": "get_public_info",
        "params": (),
        "id": 1
    });
    let response = server
        .post("/rpc")
        .authorization_bearer("not-a-valid-token")
        .json(&request_body)
        .await;
    assert_eq!(response.status_code().as_u16(), 401);
    let response: Value = response.json();
    assert_eq!(response["error"]["code"], -32001);

    // Test echo_complex
    let complex_data = json!({
        "data": [
            {"id": 1, "value": "test", "active": true},
            {"id": 2, "value": "test2", "active": false}
        ],
        "metadata": {
            "version": "1.0",
            "tags": ["test", "demo"]
        }
    });

    let response = make_jsonrpc_request(&server, "echo_complex", complex_data.clone(), None).await;

    assert_eq!(response["result"], complex_data);
}

#[tokio::test]
async fn test_authentication_required_methods() {
    let server = create_test_server();

    // Test without token - should fail
    let response = make_jsonrpc_request(&server, "sign_out", json!(()), None).await;

    assert!(response.get("error").is_some());
    let error = &response["error"];
    assert_eq!(error["code"], -32001); // Custom auth error code

    // Test with valid token - should succeed
    let response =
        make_jsonrpc_request(&server, "sign_out", json!(()), Some("valid-admin-token")).await;

    assert!(response.get("error").is_none());
    assert_eq!(response["result"], json!(()));

    // Test get_user_info with valid token
    let response = make_jsonrpc_request(
        &server,
        "get_user_info",
        json!(()),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "User regular-user");
    assert_eq!(result["email"], "regular-user@test.com");

    // Test process_data
    let response = make_jsonrpc_request(
        &server,
        "process_data",
        json!(["item1", "item2", "item3"]),
        Some("valid-empty-perms-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["processed_count"], 3);
    assert_eq!(result["success"].as_bool(), Some(true));
}

#[tokio::test]
async fn test_cookie_auth_coexists_with_bearer_tokens() {
    let server = create_cookie_test_server();
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": "get_user_info",
        "params": (),
        "id": 1
    });

    // Cookie auth on a POST requires the double-submit CSRF header.
    let response: Value = server
        .post("/rpc")
        .add_header(
            "Cookie",
            "__Host-ras-session=valid-user-token; __Host-ras-csrf=csrf-token",
        )
        .add_header("x-ras-csrf", "csrf-token")
        .json(&request_body)
        .await
        .json();

    assert_eq!(response["result"]["name"], "User regular-user");

    let response: Value = server
        .post("/rpc")
        .authorization_bearer("valid-admin-token")
        .add_header("Cookie", "__Host-ras-session=valid-user-token")
        .json(&request_body)
        .await
        .json();

    assert_eq!(response["result"]["name"], "User admin-user");

    let response = server
        .post("/rpc")
        .add_header("Authorization", "Basic invalid")
        .add_header("Cookie", "__Host-ras-session=valid-user-token")
        .json(&request_body)
        .await;

    assert_eq!(response.status_code().as_u16(), 401);
    let response: Value = response.json();
    assert_eq!(response["error"]["code"], -32001);
}

#[tokio::test]
async fn test_cookie_auth_csrf_guard_for_jsonrpc_posts() {
    let server = create_cookie_test_server();
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": "get_user_info",
        "params": (),
        "id": 1
    });

    let response = server
        .post("/rpc")
        .add_header("Cookie", "__Host-ras-session=valid-user-token")
        .json(&request_body)
        .await;

    assert_eq!(response.status_code().as_u16(), 403);
    let response: Value = response.json();
    assert_eq!(response["error"]["code"], -32004);

    let response = server
        .post("/rpc")
        .add_header(
            "Cookie",
            "__Host-ras-session=valid-user-token; __Host-ras-csrf=csrf-token",
        )
        .add_header("x-ras-csrf", "csrf-token")
        .json(&request_body)
        .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let response: Value = response.json();
    assert_eq!(response["result"]["name"], "User regular-user");

    let response = server
        .post("/rpc")
        .authorization_bearer("valid-user-token")
        .json(&request_body)
        .await;

    assert_eq!(response.status_code().as_u16(), 200);
}

#[tokio::test]
async fn test_admin_permission_methods() {
    let server = create_test_server();

    // Test with user token (insufficient permissions) - should fail
    let response = make_jsonrpc_request(
        &server,
        "delete_everything",
        json!(()),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_some());
    let error = &response["error"];
    assert_eq!(error["code"], -32002); // Insufficient permissions error

    // Test with admin token - should succeed
    let response = make_jsonrpc_request(
        &server,
        "delete_everything",
        json!(()),
        Some("valid-admin-token"),
    )
    .await;

    assert!(response.get("error").is_none());

    // Test create_user with admin token
    let response = make_jsonrpc_request(
        &server,
        "create_user",
        json!({
            "name": "New User",
            "email": "new@test.com",
            "permissions": ["user"]
        }),
        Some("valid-admin-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "New User");
    assert_eq!(result["email"], "new@test.com");
    assert!(result["id"].as_i64().unwrap() >= 1000);
}

#[tokio::test]
async fn test_user_permission_methods() {
    let server = create_test_server();

    // Test with empty permissions token - should fail
    let response = make_jsonrpc_request(
        &server,
        "update_profile",
        json!({
            "name": "Updated User",
            "email": "updated@test.com",
            "permissions": []
        }),
        Some("valid-empty-perms-token"),
    )
    .await;

    assert!(response.get("error").is_some());

    // Test with user token - should succeed
    let response = make_jsonrpc_request(
        &server,
        "update_profile",
        json!({
            "name": "Updated User",
            "email": "updated@test.com",
            "permissions": []
        }),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "Updated User");
    assert_eq!(result["id"], 456);

    // Test get_user_data with existing user
    let response = make_jsonrpc_request(
        &server,
        "get_user_data",
        json!(123),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    let result = &response["result"];
    assert_eq!(result["name"], "Found User");

    // Test get_user_data with non-existing user
    let response = make_jsonrpc_request(
        &server,
        "get_user_data",
        json!(999),
        Some("valid-user-token"),
    )
    .await;

    assert!(response.get("error").is_none());
    assert_eq!(response["result"], json!(null));
}
