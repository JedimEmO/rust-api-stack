use super::*;

#[tokio::test]
async fn test_unauthorized_endpoints() {
    let server = create_rest_test_server();

    // Test GET /api/v1/users without auth
    let response = make_rest_request(&server, Method::GET, "/api/v1/users", None, None).await;

    assert_eq!(response.status_code().as_u16(), 200);
    let users_response: UsersResponse = response.json();
    assert_eq!(users_response.total, 2);
    assert_eq!(users_response.users.len(), 2);
    assert_eq!(users_response.users[0].name, "John Doe");

    // Test GET /api/v1/users/123/posts without auth
    let response =
        make_rest_request(&server, Method::GET, "/api/v1/users/123/posts", None, None).await;

    assert_eq!(response.status_code().as_u16(), 200);
    let posts_response: PostsResponse = response.json();
    assert_eq!(posts_response.total, 1);
    assert_eq!(posts_response.posts[0].user_id, 123);

    // Test GET /api/v1/health
    let response = make_rest_request(&server, Method::GET, "/api/v1/health", None, None).await;

    assert_eq!(response.status_code().as_u16(), 200);
    let health: String = response.json();
    assert_eq!(health, "OK");
}

#[tokio::test]
async fn test_authentication_required_endpoints() {
    let server = create_rest_test_server();

    // Test GET /api/v1/status without token - should fail
    let response = make_rest_request(&server, Method::GET, "/api/v1/status", None, None).await;

    assert_eq!(response.status_code().as_u16(), 401);

    // Test GET /api/v1/status with valid token - should succeed
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/status",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let status: Value = response.json();
    assert_eq!(status["status"], "authenticated");
    assert_eq!(status["user_id"], "regular-user");

    // Test GET /api/v1/users/123/posts/456 with valid token
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/123/posts/456",
        None,
        Some("empty-perms-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let post: Post = response.json();
    assert_eq!(post.id, Some(456));
    assert_eq!(post.user_id, 123);
    assert_eq!(post.title, "Protected Post");
}

#[tokio::test]
async fn test_cookie_auth_coexists_with_bearer_tokens() {
    let server = create_rest_cookie_test_server(false);

    let response = server
        .get("/api/v1/status")
        .add_header("Cookie", "__Host-ras-session=user-token")
        .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let status: Value = response.json();
    assert_eq!(status["user_id"], "regular-user");

    let response = server
        .get("/api/v1/status")
        .authorization_bearer("admin-token")
        .add_header("Cookie", "__Host-ras-session=user-token")
        .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let status: Value = response.json();
    assert_eq!(status["user_id"], "admin-user");

    let response = server
        .get("/api/v1/status")
        .add_header("Authorization", "Basic invalid")
        .add_header("Cookie", "__Host-ras-session=user-token")
        .await;

    assert_eq!(response.status_code().as_u16(), 401);
}

#[tokio::test]
async fn test_cookie_auth_csrf_guard_only_applies_to_cookie_unsafe_requests() {
    let server = create_rest_cookie_test_server(true);
    let create_user = json!({
        "name": "Cookie User",
        "email": "cookie@example.com",
        "permissions": ["user"]
    });

    let response = server
        .post("/api/v1/users")
        .add_header("Cookie", "__Host-ras-session=admin-token")
        .json(&create_user)
        .await;

    assert_eq!(response.status_code().as_u16(), 403);

    let response = server
        .post("/api/v1/users")
        .add_header(
            "Cookie",
            "__Host-ras-session=admin-token; __Host-ras-csrf=csrf-token",
        )
        .add_header("x-ras-csrf", "csrf-token")
        .json(&create_user)
        .await;

    assert_eq!(response.status_code().as_u16(), 201);

    let response = server
        .post("/api/v1/users")
        .authorization_bearer("admin-token")
        .json(&create_user)
        .await;

    assert_eq!(response.status_code().as_u16(), 201);
}

#[tokio::test]
async fn test_admin_permission_endpoints() {
    let server = create_rest_test_server();

    // Test POST /api/v1/users with user token (insufficient permissions) - should fail
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/users",
        Some(json!({
            "name": "New User",
            "email": "new@example.com",
            "permissions": ["user"]
        })),
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 403);

    // Test POST /api/v1/users with admin token - should succeed
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/users",
        Some(json!({
            "name": "New User",
            "email": "new@example.com",
            "permissions": ["user"]
        })),
        Some("admin-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 201); // Created
    let user: User = response.json();
    assert_eq!(user.name, "New User");
    assert_eq!(user.email, "new@example.com");
    assert!(user.id.unwrap() >= 100);

    // Test PUT /api/v1/users/123 with admin token
    let response = make_rest_request(
        &server,
        Method::PUT,
        "/api/v1/users/123",
        Some(json!({
            "name": "Updated User",
            "email": "updated@example.com"
        })),
        Some("admin-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let user: User = response.json();
    assert_eq!(user.id, Some(123));
    assert_eq!(user.name, "Updated User");

    // Test DELETE /api/v1/users/123 with admin token
    let response = make_rest_request(
        &server,
        Method::DELETE,
        "/api/v1/users/123",
        None,
        Some("admin-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 204); // No Content
}

#[tokio::test]
async fn test_user_permission_endpoints() {
    let server = create_rest_test_server();

    // Test GET /api/v1/users/123 with empty permissions token - should fail
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/123",
        None,
        Some("empty-perms-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 403);

    // Test GET /api/v1/users/123 with user token - should succeed
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/123",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let user: User = response.json();
    assert_eq!(user.id, Some(123));
    assert_eq!(user.name, "Found User");

    // Test GET /api/v1/users/404 with user token - should return error
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/404",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 404); // Not Found

    // Test POST /api/v1/users/123/posts with user token
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/users/123/posts",
        Some(json!({
            "title": "My New Post",
            "content": "This is my new post content",
            "tags": ["personal", "test"]
        })),
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 201); // Created
    let post: Post = response.json();
    assert_eq!(post.user_id, 123);
    assert_eq!(post.title, "My New Post");
    assert!(!post.published);
}

#[tokio::test]
async fn test_multiple_permissions_endpoints() {
    let server = create_rest_test_server();

    // Test PUT /api/v1/users/123/posts/456 with user token - should fail (needs both "user" AND "moderator")
    let response = make_rest_request(
        &server,
        Method::PUT,
        "/api/v1/users/123/posts/456",
        Some(json!({
            "title": "Updated Post",
            "content": "Updated content",
            "tags": ["updated"]
        })),
        Some("user-token"),
    )
    .await;

    assert_ne!(response.status_code().as_u16(), 200);

    // Test PUT /api/v1/users/123/posts/456 with moderator token - should succeed (has both "user" and "moderator")
    let response = make_rest_request(
        &server,
        Method::PUT,
        "/api/v1/users/123/posts/456",
        Some(json!({
            "title": "Moderator Updated Post",
            "content": "Moderator updated content",
            "tags": ["moderated"]
        })),
        Some("moderator-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);

    let post: Post = response.json();
    assert_eq!(post.title, "Moderator Updated Post");

    // Test PUT /api/v1/users/123/posts/456 with empty permissions - should fail
    let response = make_rest_request(
        &server,
        Method::PUT,
        "/api/v1/users/123/posts/456",
        Some(json!({
            "title": "Unauthorized Update",
            "content": "Should not work",
            "tags": []
        })),
        Some("empty-perms-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 403);

    // Test DELETE /api/v1/users/123/posts/456 with admin token - should succeed
    let response = make_rest_request(
        &server,
        Method::DELETE,
        "/api/v1/users/123/posts/456",
        None,
        Some("admin-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 204); // No Content

    // Test DELETE /api/v1/users/123/posts/456 with moderator token - should succeed
    let response = make_rest_request(
        &server,
        Method::DELETE,
        "/api/v1/users/123/posts/456",
        None,
        Some("moderator-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 204); // No Content
}

#[tokio::test]
async fn test_new_permission_logic() {
    let server = create_rest_test_server();

    // Test admin_action endpoint with new permission logic:
    // WITH_PERMISSIONS(["admin", "moderator"] | ["super_user"])
    // This means user needs (admin AND moderator) OR (super_user)

    // Test with admin-token (has "admin" and "user", but NOT "moderator") - should FAIL
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/admin_action",
        Some(serde_json::Value::Null), // Send null for unit type
        Some("admin-token"),
    )
    .await;
    assert_eq!(
        response.status_code().as_u16(),
        403,
        "Admin token should fail - has admin but not moderator"
    );

    // Test with moderator-token (has "moderator" and "user", but NOT "admin") - should FAIL
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/admin_action",
        Some(Value::Null), // Send null for unit type
        Some("moderator-token"),
    )
    .await;
    assert_eq!(
        response.status_code().as_u16(),
        403,
        "Moderator token should fail - has moderator but not admin"
    );

    // Test with superuser-token (has "superuser" and "admin") - should SUCCEED
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/admin_action",
        Some(Value::Null), // Send null for unit type
        Some("superuser-token"),
    )
    .await;
    assert_eq!(
        response.status_code().as_u16(),
        200,
        "superuser should succeed"
    );

    // We would need a token with both admin AND moderator permissions to test success
    // But our test auth provider doesn't have such a token

    // The DELETE endpoint uses ["moderator"] | ["admin"] - should succeed with either
    // Test with admin-token (has "admin") - should SUCCEED
    let response = make_rest_request(
        &server,
        Method::DELETE,
        "/api/v1/users/123/posts/456",
        None,
        Some("admin-token"),
    )
    .await;
    assert_eq!(
        response.status_code().as_u16(),
        204, // No Content
        "Admin token should succeed for delete - has admin"
    );

    // Test with moderator-token (has "moderator") - should SUCCEED
    let response = make_rest_request(
        &server,
        Method::DELETE,
        "/api/v1/users/123/posts/456",
        None,
        Some("moderator-token"),
    )
    .await;
    assert_eq!(
        response.status_code().as_u16(),
        204, // No Content
        "Moderator token should succeed for delete - has moderator"
    );
}

#[tokio::test]
async fn test_body_is_not_parsed_before_auth() {
    let server = create_rest_test_server();

    // Invalid JSON without credentials must be rejected by auth (401, not
    // 400), proving the body is neither read nor parsed before the
    // auth/CSRF/permission checks succeed.
    let response = server
        .post("/api/v1/users")
        .text("{invalid json")
        .content_type("application/json")
        .await;
    assert_eq!(response.status_code().as_u16(), 401);

    // Same body with an invalid token: still rejected by auth.
    let response = server
        .post("/api/v1/users")
        .authorization_bearer("wrong-token")
        .text("{invalid json")
        .content_type("application/json")
        .await;
    assert_eq!(response.status_code().as_u16(), 401);

    // Valid credentials allow body parsing, which rejects the malformed payload.
    let response = server
        .post("/api/v1/users")
        .authorization_bearer("admin-token")
        .text("{invalid json")
        .content_type("application/json")
        .await;
    assert_eq!(response.status_code().as_u16(), 400);
}
