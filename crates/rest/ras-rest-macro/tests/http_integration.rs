use axum::http::Method;
use axum_test::{TestResponse, TestServer};
use rand::Rng;
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_rest_core::{RestError, RestResponse};
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

// Test data structures for REST API testing
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct User {
    id: Option<i32>,
    name: String,
    email: String,
    permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct CreateUserRequest {
    name: String,
    email: String,
    permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct UpdateUserRequest {
    name: String,
    email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct UsersResponse {
    users: Vec<User>,
    total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct PostRequest {
    title: String,
    content: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Post {
    id: Option<i32>,
    user_id: i32,
    title: String,
    content: String,
    tags: Vec<String>,
    published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct PostsResponse {
    posts: Vec<Post>,
    total: usize,
}

// Simple test auth provider
struct TestRestAuthProvider {
    valid_tokens: HashSet<String>,
}

impl TestRestAuthProvider {
    fn new() -> Self {
        let mut valid_tokens = HashSet::new();
        valid_tokens.insert("admin-token".to_string());
        valid_tokens.insert("user-token".to_string());
        valid_tokens.insert("moderator-token".to_string());
        valid_tokens.insert("superuser-token".to_string());
        valid_tokens.insert("empty-perms-token".to_string());

        Self { valid_tokens }
    }
}

impl AuthProvider for TestRestAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if !self.valid_tokens.contains(&token) {
                return Err(AuthError::InvalidToken);
            }

            let (user_id, permissions) = match token.as_str() {
                "admin-token" => ("admin-user", vec!["admin".to_string(), "user".to_string()]),
                "superuser-token" => (
                    "superuser-user",
                    vec!["admin".to_string(), "super_user".to_string()],
                ),
                "user-token" => ("regular-user", vec!["user".to_string()]),
                "moderator-token" => (
                    "mod-user",
                    vec!["moderator".to_string(), "user".to_string()],
                ),
                "empty-perms-token" => ("guest-user", vec![]),
                _ => return Err(AuthError::InvalidToken),
            };

            Ok(AuthenticatedUser {
                user_id: user_id.to_string(),
                permissions: permissions.into_iter().collect(),
                metadata: None,
            })
        })
    }
}

// Generate a broad REST test service
rest_service!({
    service_name: TestRestService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    ui_theme: "default",
    endpoints: [
        // User management endpoints
        /// List users.
        ///
        /// Returns all users visible to the caller.
        GET UNAUTHORIZED users() -> UsersResponse,
        /// Create a user.
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
        GET WITH_PERMISSIONS(["user"]) users/{id: i32}() -> User,
        PUT WITH_PERMISSIONS(["admin"]) users/{id: i32}(UpdateUserRequest) -> User,
        DELETE WITH_PERMISSIONS(["admin"]) users/{id: i32}() -> (),

        // Posts endpoints with nested paths
        GET UNAUTHORIZED users/{user_id: i32}/posts() -> PostsResponse,
        POST WITH_PERMISSIONS(["user"]) users/{user_id: i32}/posts(PostRequest) -> Post,
        GET WITH_PERMISSIONS([]) users/{user_id: i32}/posts/{post_id: i32}() -> Post,
        PUT WITH_PERMISSIONS(["user", "moderator"]) users/{user_id: i32}/posts/{post_id: i32}(PostRequest) -> Post,
        DELETE WITH_PERMISSIONS(["moderator"] | ["admin"]) users/{user_id: i32}/posts/{post_id: i32}() -> (),

        // Health check and status endpoints
        GET UNAUTHORIZED health() -> String,
        GET WITH_PERMISSIONS([]) status() -> Value,

        // OR syntax demonstration endpoint
        POST WITH_PERMISSIONS(["admin", "moderator"] | ["super_user"]) admin_action(()) -> String,

        // Query parameter test endpoints
        GET UNAUTHORIZED search/users ? q: String & limit: Option<u32> & offset: Option<u32> () -> UsersResponse,
        GET WITH_PERMISSIONS(["user"]) search/posts ? tag: Option<String> & published: Option<bool> () -> PostsResponse,
        POST WITH_PERMISSIONS(["admin"]) users/batch ? notify: bool (CreateUserRequest) -> User,
        GET UNAUTHORIZED posts/paginated ? page: u32 & per_page: Option<u32> () -> PostsResponse,
    ]
});

// Test service implementation
struct TestRestServiceImpl;

#[async_trait::async_trait]
impl TestRestServiceTrait for TestRestServiceImpl {
    async fn get_users(&self) -> ras_rest_core::RestResult<UsersResponse> {
        Ok(RestResponse::ok(UsersResponse {
            users: vec![
                User {
                    id: Some(1),
                    name: "John Doe".to_string(),
                    email: "john@example.com".to_string(),
                    permissions: vec!["user".to_string()],
                },
                User {
                    id: Some(2),
                    name: "Jane Admin".to_string(),
                    email: "jane@example.com".to_string(),
                    permissions: vec!["admin".to_string()],
                },
            ],
            total: 2,
        }))
    }

    async fn post_users(
        &self,
        _user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> ras_rest_core::RestResult<User> {
        Ok(RestResponse::created(User {
            id: Some(rand::thread_rng().gen_range(100..999)),
            name: request.name,
            email: request.email,
            permissions: request.permissions,
        }))
    }

    async fn get_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        id: i32,
    ) -> ras_rest_core::RestResult<User> {
        if id == 404 {
            Err(RestError::not_found("User not found"))
        } else {
            Ok(RestResponse::ok(User {
                id: Some(id),
                name: "Found User".to_string(),
                email: "found@example.com".to_string(),
                permissions: vec!["user".to_string()],
            }))
        }
    }

    async fn put_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        id: i32,
        request: UpdateUserRequest,
    ) -> ras_rest_core::RestResult<User> {
        Ok(RestResponse::ok(User {
            id: Some(id),
            name: request.name,
            email: request.email,
            permissions: vec!["user".to_string()],
        }))
    }

    async fn delete_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        _id: i32,
    ) -> ras_rest_core::RestResult<()> {
        Ok(RestResponse::no_content())
    }

    async fn get_users_by_user_id_posts(
        &self,
        user_id: i32,
    ) -> ras_rest_core::RestResult<PostsResponse> {
        Ok(RestResponse::ok(PostsResponse {
            posts: vec![Post {
                id: Some(1),
                user_id,
                title: "Test Post".to_string(),
                content: "This is a test post".to_string(),
                tags: vec!["test".to_string()],
                published: true,
            }],
            total: 1,
        }))
    }

    async fn post_users_by_user_id_posts(
        &self,
        _user: &AuthenticatedUser,
        user_id: i32,
        request: PostRequest,
    ) -> ras_rest_core::RestResult<Post> {
        Ok(RestResponse::created(Post {
            id: Some(rand::thread_rng().gen_range(100..999)),
            user_id,
            title: request.title,
            content: request.content,
            tags: request.tags,
            published: false,
        }))
    }

    async fn get_users_by_user_id_posts_by_post_id(
        &self,
        _user: &AuthenticatedUser,
        user_id: i32,
        post_id: i32,
    ) -> ras_rest_core::RestResult<Post> {
        Ok(RestResponse::ok(Post {
            id: Some(post_id),
            user_id,
            title: "Protected Post".to_string(),
            content: "This requires authentication".to_string(),
            tags: vec!["protected".to_string()],
            published: true,
        }))
    }

    async fn put_users_by_user_id_posts_by_post_id(
        &self,
        _user: &AuthenticatedUser,
        user_id: i32,
        post_id: i32,
        request: PostRequest,
    ) -> ras_rest_core::RestResult<Post> {
        Ok(RestResponse::ok(Post {
            id: Some(post_id),
            user_id,
            title: request.title,
            content: request.content,
            tags: request.tags,
            published: true,
        }))
    }

    async fn delete_users_by_user_id_posts_by_post_id(
        &self,
        _user: &AuthenticatedUser,
        _user_id: i32,
        _post_id: i32,
    ) -> ras_rest_core::RestResult<()> {
        Ok(RestResponse::no_content())
    }

    async fn get_health(&self) -> ras_rest_core::RestResult<String> {
        Ok(RestResponse::ok("OK".to_string()))
    }

    async fn get_status(&self, user: &AuthenticatedUser) -> ras_rest_core::RestResult<Value> {
        let value = json!({
            "status": "authenticated",
            "user_id": user.user_id,
            "permissions": user.permissions.iter().collect::<Vec<_>>(),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        Ok(RestResponse::ok(value))
    }

    async fn post_admin_action(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> ras_rest_core::RestResult<String> {
        Ok(RestResponse::ok("Admin action completed".to_string()))
    }

    // Query parameter test implementations
    async fn get_search_users(
        &self,
        q: String,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> ras_rest_core::RestResult<UsersResponse> {
        let limit = limit.unwrap_or(10);
        let offset = offset.unwrap_or(0);

        // Filter users based on search query
        let users = vec![User {
            id: Some(1),
            name: format!("User matching '{}' at offset {}", q, offset),
            email: "search@example.com".to_string(),
            permissions: vec!["user".to_string()],
        }];

        Ok(RestResponse::ok(UsersResponse {
            users: users.into_iter().take(limit as usize).collect(),
            total: 1,
        }))
    }

    async fn get_search_posts(
        &self,
        _user: &AuthenticatedUser,
        tag: Option<String>,
        published: Option<bool>,
    ) -> ras_rest_core::RestResult<PostsResponse> {
        let mut posts = vec![Post {
            id: Some(1),
            user_id: 1,
            title: "Test Post".to_string(),
            content: "Content".to_string(),
            tags: vec!["test".to_string()],
            published: true,
        }];

        // Filter by tag if provided
        if let Some(tag) = tag {
            posts.retain(|p| p.tags.contains(&tag));
        }

        // Filter by published status if provided
        if let Some(published) = published {
            posts.retain(|p| p.published == published);
        }

        Ok(RestResponse::ok(PostsResponse {
            total: posts.len(),
            posts,
        }))
    }

    async fn post_users_batch(
        &self,
        _user: &AuthenticatedUser,
        notify: bool,
        request: CreateUserRequest,
    ) -> ras_rest_core::RestResult<User> {
        // The notify parameter could trigger notifications
        if notify {
            // In a real implementation, send notification
            tracing::info!("Notification would be sent for new user: {}", request.name);
        }

        Ok(RestResponse::created(User {
            id: Some(rand::thread_rng().gen_range(100..999)),
            name: request.name,
            email: request.email,
            permissions: request.permissions,
        }))
    }

    async fn get_posts_paginated(
        &self,
        page: u32,
        per_page: Option<u32>,
    ) -> ras_rest_core::RestResult<PostsResponse> {
        let per_page = per_page.unwrap_or(20);
        let start = (page - 1) * per_page;

        // Generate paginated posts
        let posts: Vec<Post> = (start..start + per_page)
            .map(|i| Post {
                id: Some(i as i32),
                user_id: 1,
                title: format!("Post {}", i),
                content: format!("Content for post {}", i),
                tags: vec!["paginated".to_string()],
                published: true,
            })
            .collect();

        Ok(RestResponse::ok(PostsResponse {
            total: 100, // Mock total
            posts,
        }))
    }
}

fn create_rest_test_server() -> TestServer {
    let builder =
        TestRestServiceBuilder::new(TestRestServiceImpl).auth_provider(TestRestAuthProvider::new());

    let app = builder.build();
    TestServer::builder().mock_transport().build(app).unwrap()
}

async fn make_rest_request(
    server: &TestServer,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> TestResponse {
    let mut request = match method {
        Method::GET => server.get(path),
        Method::POST => server.post(path),
        Method::PUT => server.put(path),
        Method::PATCH => server.patch(path),
        Method::DELETE => server.delete(path),
        other => panic!("unsupported test method: {other}"),
    };

    if let Some(token) = token {
        request = request.authorization_bearer(token);
    }

    if let Some(body) = body {
        request.json(&body).await
    } else {
        request.await
    }
}

#[tokio::test]
async fn test_docs_explorer_routes_generated() {
    let server = create_rest_test_server();

    let docs_response = server.get("/api/v1/docs").await;
    assert_eq!(docs_response.status_code().as_u16(), 200);

    let docs = docs_response.text();
    assert!(docs.contains("\"TestRestService\""));
    assert!(docs.contains("\"rest\""));
    assert!(docs.contains("/api/v1/docs/openapi.json"));
    assert!(docs.contains("id=\"bearer-token\""));
    assert!(docs.contains("id=\"saved-list\""));

    let spec_response = server.get("/api/v1/docs/openapi.json").await;
    assert_eq!(spec_response.status_code().as_u16(), 200);

    let spec: serde_json::Value = spec_response.json();
    assert_eq!(spec["info"]["title"], "TestRestService REST API");
    assert!(spec["paths"].is_object());
}

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
async fn test_openapi_generation() {
    let _ = TestRestServiceBuilder::new(TestRestServiceImpl);

    let openapi_doc = generate_testrestservice_openapi();
    assert_eq!(openapi_doc["openapi"], "3.0.3");

    let get_users = &openapi_doc["paths"]["/users"]["get"];
    assert_eq!(get_users["summary"], "List users.");
    assert_eq!(
        get_users["description"],
        "List users.\n\nReturns all users visible to the caller."
    );

    let post_users = &openapi_doc["paths"]["/users"]["post"];
    assert_eq!(post_users["summary"], "Create a user.");
    assert_eq!(post_users["description"], "Create a user.");

    let health = &openapi_doc["paths"]["/health"]["get"];
    assert_eq!(health["summary"], "GET /health");
    assert_eq!(health["description"], "Handles GET requests to /health");
}

#[tokio::test]
async fn test_missing_dependencies() {
    // Import futures for the join_all function
    use futures::future::join_all;

    // This test ensures that our future handling is working correctly
    let handles: Vec<tokio::task::JoinHandle<()>> = vec![];
    let _results = join_all(handles).await;
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
async fn test_generated_rest_client() {
    let mut client = TestRestServiceClientBuilder::new("http://example.invalid")
        .with_timeout(std::time::Duration::from_millis(100))
        .build()
        .unwrap();

    client.set_bearer_token(Some("superuser-token"));
    assert_eq!(client.bearer_token(), Some("superuser-token"));
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
