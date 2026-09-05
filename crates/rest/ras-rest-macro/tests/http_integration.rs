use axum::http::Method;
use axum_test::{TestResponse, TestServer};
use rand::Rng;
use ras_jsonrpc_core::{
    AuthCookieConfig, AuthError, AuthFuture, AuthProvider, AuthenticatedUser, CsrfConfig,
};
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

/// `Arc`-wrapped twin of [`create_rest_test_server`] for sharing with an
/// in-process [`AxumTestTransport`].
fn create_rest_test_server_arc() -> Arc<TestServer> {
    Arc::new(create_rest_test_server())
}

/// Build a generated `TestRestServiceClient` over an in-process transport
/// backed by the shared `TestServer`.
fn create_rest_test_client(server: Arc<TestServer>) -> TestRestServiceClient {
    let transport: Arc<dyn ras_transport_core::HttpTransport> =
        Arc::new(ras_transport_core::AxumTestTransport::from_arc(server));
    TestRestServiceClientBuilder::new("http://in-memory.test")
        .build_with_transport(transport)
        .expect("failed to build TestRestServiceClient over AxumTestTransport")
}

fn create_rest_cookie_test_server(csrf: bool) -> TestServer {
    let mut builder = TestRestServiceBuilder::new(TestRestServiceImpl)
        .auth_provider(TestRestAuthProvider::new())
        .auth_cookie(AuthCookieConfig::default());

    if csrf {
        builder = builder.csrf_protection(CsrfConfig::default());
    }

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

// Minimal service exercising the body_limit option.
rest_service!({
    service_name: TinyBodyService,
    base_path: "/tiny",
    body_limit: 64,
    endpoints: [
        POST UNAUTHORIZED echo(Value) -> Value,
    ]
});

struct TinyBodyServiceImpl;

#[async_trait::async_trait]
impl TinyBodyServiceTrait for TinyBodyServiceImpl {
    async fn post_echo(&self, request: Value) -> ras_rest_core::RestResult<Value> {
        Ok(RestResponse::ok(request))
    }
}

#[path = "http_integration/auth.rs"]
mod auth;
#[path = "http_integration/client.rs"]
mod client;
#[path = "http_integration/errors.rs"]
mod errors;
#[path = "http_integration/parameters.rs"]
mod parameters;
#[path = "http_integration/specs.rs"]
mod specs;
