use super::*;

#[tokio::test]
async fn test_invalid_requests() {
    let server = create_rest_test_server();

    // Test non-existent endpoint
    let response = make_rest_request(&server, Method::GET, "/api/v1/nonexistent", None, None).await;

    assert_eq!(response.status_code().as_u16(), 404);

    // Test invalid HTTP method
    let response = make_rest_request(&server, Method::PATCH, "/api/v1/users", None, None).await;

    assert_eq!(response.status_code().as_u16(), 405);

    // Test invalid JSON body
    let response = server
        .post("/api/v1/users")
        .authorization_bearer("admin-token")
        .text("{invalid json")
        .content_type("application/json")
        .await;

    assert_eq!(response.status_code().as_u16(), 400);

    // Test missing required fields
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/users",
        Some(json!({
            "name": "Incomplete User"
            // Missing email and permissions
        })),
        Some("admin-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 400);
}

#[tokio::test]
async fn test_path_parameters() {
    let server = create_rest_test_server();

    // Test single path parameter
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/42",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let user: User = response.json();
    assert_eq!(user.id, Some(42));

    // Test multiple path parameters
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/123/posts/789",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let post: Post = response.json();
    assert_eq!(post.user_id, 123);
    assert_eq!(post.id, Some(789));

    // Test path parameters with request body
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/users/999/posts",
        Some(json!({
            "title": "Path Param Post",
            "content": "Testing path parameters with body",
            "tags": ["path", "test"]
        })),
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 201); // Created
    let post: Post = response.json();
    assert_eq!(post.user_id, 999);
    assert_eq!(post.title, "Path Param Post");
}

#[tokio::test]
async fn test_query_parameters() {
    let server = create_rest_test_server();

    // Test search with required and optional query parameters
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/search/users?q=john&limit=5&offset=10",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let users_response: UsersResponse = response.json();
    assert!(users_response.users[0].name.contains("john"));
    assert!(users_response.users[0].name.contains("offset 10"));

    // Test with only required parameter
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/search/users?q=jane",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let users_response: UsersResponse = response.json();
    assert!(users_response.users[0].name.contains("jane"));

    // Test missing required parameter - should fail
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/search/users?limit=5",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 400); // Bad Request
}

#[tokio::test]
async fn test_query_parameters_with_auth() {
    let server = create_rest_test_server();

    // Test search posts with optional query parameters and authentication
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/search/posts?tag=test&published=true",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let posts_response: PostsResponse = response.json();
    assert!(posts_response.posts[0].tags.contains(&"test".to_string()));
    assert!(posts_response.posts[0].published);

    // Test with no query parameters - all optional
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/search/posts",
        None,
        Some("user-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
}

#[tokio::test]
async fn test_query_parameters_with_body() {
    let server = create_rest_test_server();

    // Test POST with query parameter and request body
    let response = make_rest_request(
        &server,
        Method::POST,
        "/api/v1/users/batch?notify=true",
        Some(json!({
            "name": "New User",
            "email": "new@example.com",
            "permissions": ["user"]
        })),
        Some("admin-token"),
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 201);
    let user: User = response.json();
    assert_eq!(user.name, "New User");
}

#[tokio::test]
async fn test_query_parameters_with_path_params() {
    let server = create_rest_test_server();

    // Test endpoint with query parameters
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/posts/paginated?page=2&per_page=5",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let posts_response: PostsResponse = response.json();
    assert_eq!(posts_response.posts.len(), 5);
    assert_eq!(posts_response.posts[0].user_id, 1);

    // Test with only required query parameter
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/posts/paginated?page=1",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 200);
    let posts_response: PostsResponse = response.json();
    assert_eq!(posts_response.posts.len(), 20); // Default per_page
}

#[tokio::test]
async fn test_body_limit_option_enforced() {
    let app = TinyBodyServiceBuilder::new(TinyBodyServiceImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.post("/tiny/echo").json(&json!({"ok": true})).await;
    assert_eq!(response.status_code().as_u16(), 200);

    let response = server
        .post("/tiny/echo")
        .json(&json!({"data": "x".repeat(200)}))
        .await;
    assert_eq!(response.status_code().as_u16(), 413);
}

/// F2: axum's default `Path` rejection echoes the offending value (e.g.
/// "Cannot parse `abc` to a `i32`"); the generated handler must return a fixed
/// JSON message instead and log the detail server-side.
#[tokio::test]
async fn f2_invalid_path_parameter_returns_generic_json_error() {
    let server = create_rest_test_server();

    // UNAUTHORIZED route with an `i32` path param.
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/users/not-an-int-9f3c/posts",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 400);
    let text = response.text();
    assert!(
        !text.contains("not-an-int-9f3c"),
        "path value echoed to client: {text}"
    );
    assert!(
        !text.contains("i32"),
        "type detail echoed to client: {text}"
    );
    let body: Value = response.json();
    assert_eq!(body["error"], "Invalid path parameters");
}

/// F2: same for query-string rejections from `axum_extra::extract::Query`.
#[tokio::test]
async fn f2_invalid_query_parameter_returns_generic_json_error() {
    let server = create_rest_test_server();

    // `page: u32` — a non-numeric value is rejected.
    let response = make_rest_request(
        &server,
        Method::GET,
        "/api/v1/posts/paginated?page=zz-not-a-page",
        None,
        None,
    )
    .await;

    assert_eq!(response.status_code().as_u16(), 400);
    let text = response.text();
    assert!(
        !text.contains("zz-not-a-page"),
        "query value echoed to client: {text}"
    );
    let body: Value = response.json();
    assert_eq!(body["error"], "Invalid query parameters");

    // Missing required query param is also a generic 400.
    let response =
        make_rest_request(&server, Method::GET, "/api/v1/posts/paginated", None, None).await;
    assert_eq!(response.status_code().as_u16(), 400);
    let body: Value = response.json();
    assert_eq!(body["error"], "Invalid query parameters");
}
