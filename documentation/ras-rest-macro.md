# RAS REST Macro Documentation

The `ras-rest-macro` crate provides a procedural macro for building type-safe REST APIs in Rust with generated native Rust clients and OpenAPI documents for TypeScript client generation.

## Table of Contents

1. [Overview](#overview)
2. [Installation](#installation)
3. [Basic Usage](#basic-usage)
4. [Macro Syntax](#macro-syntax)
5. [Authentication & Authorization](#authentication--authorization)
6. [Versioned Endpoints](#versioned-endpoints)
7. [Generated Code](#generated-code)
8. [TypeScript Client Usage](#typescript-client-usage)
9. [OpenAPI Documentation](#openapi-documentation)
10. [Error Handling](#error-handling)
11. [Advanced Features](#advanced-features)
12. [Task API Example](#task-api-example)

## Overview

The `rest_service!` macro generates:
- A service trait for implementing your REST API
- An Axum router builder with authentication support
- Native Rust client with async/await support
- OpenAPI 3.0 specification for TypeScript client generation
- Built-in API explorer hosting (optional)
- Optional compatibility routes that migrate legacy request/response shapes

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
ras-rest-macro = { version = "0.2.1", default-features = false }
ras-rest-core = { version = "0.1.1", optional = true }
ras-auth-core = { version = "0.1.0", optional = true }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.0.0-alpha.20"
async-trait = { version = "0.1", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
axum = { version = "0.8", optional = true }
axum-extra = { version = "0.10", features = ["query"], optional = true }
tokio = { version = "1.0", features = ["full"], optional = true }
reqwest = { version = "0.12", features = ["json"], optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["json"], optional = true }

[features]
default = ["server"]
server = [
    "ras-rest-macro/server",
    "dep:ras-rest-core",
    "dep:ras-auth-core",
    "dep:async-trait",
    "dep:axum",
    "dep:axum-extra",
    "dep:tokio",
]
client = ["ras-rest-macro/client", "dep:reqwest"]
```

## Basic Usage

### 1. Define Your API Types

All request and response types must implement `Serialize`, `Deserialize`, and `JsonSchema`:

```rust
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateUserRequest {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsersResponse {
    pub users: Vec<User>,
    pub total: usize,
}
```

### 2. Define Your REST Service

```rust
use ras_rest_macro::rest_service;

rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    endpoints: [
        // Public endpoints (no auth required)
        GET UNAUTHORIZED users() -> UsersResponse,
        GET UNAUTHORIZED users/{id: String}() -> User,
        
        // Protected endpoints (auth required)
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
        PUT WITH_PERMISSIONS(["admin"]) users/{id: String}(UpdateUserRequest) -> User,
        DELETE WITH_PERMISSIONS(["admin"]) users/{id: String}() -> (),
    ]
});
```

### 3. Implement the Generated Trait

```rust
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestError, RestResponse, RestResult};
use std::collections::HashMap;
use std::sync::Mutex;

struct UserServiceImpl {
    users: Mutex<HashMap<String, User>>,
}

impl UserServiceImpl {
    fn new() -> Self {
        Self {
            users: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl UserServiceTrait for UserServiceImpl {
    async fn get_users(&self) -> RestResult<UsersResponse> {
        let users: Vec<User> = self
            .users
            .lock()
            .expect("users lock")
            .values()
            .cloned()
            .collect();

        Ok(RestResponse::ok(UsersResponse {
            total: users.len(),
            users,
        }))
    }

    async fn get_users_by_id(&self, id: String) -> RestResult<User> {
        self.users
            .lock()
            .expect("users lock")
            .get(&id)
            .cloned()
            .map(RestResponse::ok)
            .ok_or_else(|| RestError::not_found("User not found"))
    }

    async fn post_users(
        &self,
        _user: &AuthenticatedUser,  // Auto-injected for authenticated endpoints
        request: CreateUserRequest,
    ) -> RestResult<User> {
        let mut users = self.users.lock().expect("users lock");
        let id = format!("user-{}", users.len() + 1);
        let user = User {
            id: id.clone(),
            name: request.name,
            email: request.email,
        };
        users.insert(id, user.clone());

        Ok(RestResponse::created(user))
    }

    async fn put_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        id: String,
        request: UpdateUserRequest,
    ) -> RestResult<User> {
        let mut users = self.users.lock().expect("users lock");
        let user = users
            .get_mut(&id)
            .ok_or_else(|| RestError::not_found("User not found"))?;
        user.name = request.name;
        user.email = request.email;

        Ok(RestResponse::ok(user.clone()))
    }

    async fn delete_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        id: String,
    ) -> RestResult<()> {
        let removed = self.users.lock().expect("users lock").remove(&id);
        if removed.is_some() {
            Ok(RestResponse::no_content())
        } else {
            Err(RestError::not_found("User not found"))
        }
    }
}
```

### 4. Create and Run the Server

```rust
use axum::Router;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::HashSet;

struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token != "admin-token" {
                return Err(AuthError::InvalidToken);
            }

            Ok(AuthenticatedUser {
                user_id: "admin-user".to_string(),
                permissions: HashSet::from(["admin".to_string()]),
                metadata: None,
            })
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = UserServiceImpl::new();
    let auth_provider = DemoAuthProvider;

    let api_router = UserServiceBuilder::new(service)
        .auth_provider(auth_provider)
        .with_usage_tracker(|_headers, user, method, path| {
            let method = method.to_string();
            let path = path.to_string();
            let user_id = user.map(|user| user.user_id.clone());
            async move {
                println!("{} {} user={:?}", method, path, user_id);
            }
        })
        .with_method_duration_tracker(|method, path, _user, duration| {
            let method = method.to_string();
            let path = path.to_string();
            async move {
                println!("{} {} took {:?}", method, path, duration);
            }
        })
        .build();

    let app = Router::new().merge(api_router);
    
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

## Macro Syntax

### Full Syntax

```rust
rest_service!({
    service_name: ServiceName,           // Required: Name for generated types
    base_path: "/api/v1",               // Required: Base URL path
    openapi: true,                      // Optional: Enable OpenAPI generation
    serve_docs: true,                   // Optional: Enable the built-in API explorer
    docs_path: "/docs",                 // Optional: API explorer path (default: "/docs")
    ui_theme: "dark",                   // Optional: retained for compatibility
    endpoints: [
        GET UNAUTHORIZED users() -> UsersResponse,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
    ]
});
```

Use `openapi: { output: "api.json" }` instead of `openapi: true` when you
want a custom OpenAPI output path.

### Endpoint Syntax

```
METHOD AUTH_REQUIREMENT path/{param: Type}/segments(RequestType) -> ResponseType
```

- **METHOD**: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`
- **AUTH_REQUIREMENT**: 
  - `UNAUTHORIZED` - No authentication required
  - `WITH_PERMISSIONS(["permission1", "permission2"])` - Requires all listed permissions (AND)
  - `WITH_PERMISSIONS(["perm1"] | ["perm2"])` - Requires any permission group (OR)
- **Path**: URL path with optional parameters in `{name: Type}` format
- **RequestType**: Optional request body type (omit for GET/DELETE)
- **ResponseType**: Response body type (use `()` for empty responses)

### Path Parameters

Path parameters are defined inline using `{name: Type}` syntax:

```rust
GET UNAUTHORIZED users/{id: String}() -> User,
PUT WITH_PERMISSIONS(["admin"]) posts/{post_id: i32}/comments/{comment_id: i32}(UpdateCommentRequest) -> Comment,
```

## Authentication & Authorization

### Setting Up Authentication

The macro integrates with `ras-auth-core` for authentication:

```rust
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser, AuthResult};
use std::collections::HashSet;

struct DemoAuthProvider;

impl AuthProvider for DemoAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token != "admin-token" {
                return Err(AuthError::InvalidToken);
            }

            Ok(AuthenticatedUser {
                user_id: "admin-user".to_string(),
                permissions: HashSet::from(["admin".to_string()]),
                metadata: None,
            })
        })
    }

    fn check_permissions(
        &self,
        user: &AuthenticatedUser,
        required_permissions: &[String],
    ) -> AuthResult<()> {
        let missing: Vec<String> = required_permissions
            .iter()
            .filter(|permission| !user.permissions.contains(*permission))
            .cloned()
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(AuthError::InsufficientPermissions {
                required: required_permissions.to_vec(),
                has: user.permissions.iter().cloned().collect(),
            })
        }
    }
}
```

### Permission Groups

Use OR logic between permission groups and AND logic within groups:

```rust
// Requires either admin OR (moderator AND editor)
WITH_PERMISSIONS(["admin"] | ["moderator", "editor"])
```

## Versioned Endpoints

Versioned endpoints are opt-in. The canonical route stays implemented by the generated service trait. Each legacy route declares its own path, body, response, and migration type. The generated server migrates legacy request parts into the canonical request parts, calls the canonical service method, then migrates the response body back to the legacy response type.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetV1 {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetV2 {
    pub display_name: String,
    pub notify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetResponseV1 {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameWidgetResponseV2 {
    pub display_name: String,
    pub notified: bool,
}

rest_service!({
    service_name: WidgetService,
    base_path: "/api",
    openapi: true,
    endpoints: [
        POST UNAUTHORIZED v2/widgets/{id: String}/rename(RenameWidgetV2) -> RenameWidgetResponseV2 {
            version: v2,
            versions: [
                v1 {
                    path: v1/widgets/{id: String}/rename,
                    body: RenameWidgetV1,
                    response: RenameWidgetResponseV1,
                    migration: RenameWidgetCompat,
                },
            ],
        },
    ]
});

struct RenameWidgetCompat;

impl ras_rest_core::VersionMigration<
    WidgetServicePostV2WidgetsByIdRenameV1Request,
    WidgetServicePostV2WidgetsByIdRenameV2Request,
> for RenameWidgetCompat {
    type Error = std::convert::Infallible;

    fn migrate(
        value: WidgetServicePostV2WidgetsByIdRenameV1Request,
    ) -> Result<WidgetServicePostV2WidgetsByIdRenameV2Request, Self::Error> {
        Ok(WidgetServicePostV2WidgetsByIdRenameV2Request {
            path: WidgetServicePostV2WidgetsByIdRenameV2Path { id: value.path.id },
            query: WidgetServicePostV2WidgetsByIdRenameV2Query {},
            body: RenameWidgetV2 {
                display_name: value.body.name,
                notify: false,
            },
        })
    }
}

impl ras_rest_core::VersionMigration<RenameWidgetResponseV2, RenameWidgetResponseV1>
    for RenameWidgetCompat
{
    type Error = std::convert::Infallible;

    fn migrate(value: RenameWidgetResponseV2) -> Result<RenameWidgetResponseV1, Self::Error> {
        Ok(RenameWidgetResponseV1 {
            name: value.display_name,
        })
    }
}
```

OpenAPI output includes both canonical and legacy paths. Versioned operations include `x-ras-version`, `x-ras-canonical-version`, and `x-ras-canonical-path` extensions.

## Generated Code

The macro generates several components:

### 1. Service Trait

```rust
#[async_trait::async_trait]
pub trait UserServiceTrait: Send + Sync + 'static {
    async fn get_users(&self) -> RestResult<UsersResponse>;
    async fn get_users_by_id(&self, id: String) -> RestResult<User>;
    async fn post_users(&self, user: &AuthenticatedUser, request: CreateUserRequest) -> RestResult<User>;
    async fn put_users_by_id(&self, user: &AuthenticatedUser, id: String, request: UpdateUserRequest) -> RestResult<User>;
    async fn delete_users_by_id(&self, user: &AuthenticatedUser, id: String) -> RestResult<()>;
}
```

### 2. Service Builder

```rust
impl<T: UserServiceTrait> UserServiceBuilder<T> {
    pub fn new(service: T) -> Self;
    pub fn auth_provider<A: AuthProvider>(self, provider: A) -> Self;
    pub fn with_usage_tracker<F, Fut>(self, tracker: F) -> Self;
    pub fn with_method_duration_tracker<F, Fut>(self, tracker: F) -> Self;
    pub fn build(self) -> axum::Router;
}
```

### 3. Native Rust Client

```rust
impl UserServiceClient {
    pub fn builder(server_url: impl Into<String>) -> UserServiceClientBuilder;
    pub fn set_bearer_token(&mut self, token: Option<impl Into<String>>);
    
    // Generated methods matching endpoints
    pub async fn get_users(&self) -> Result<UsersResponse, Box<dyn Error>>;
    pub async fn get_users_by_id(&self, id: String) -> Result<User, Box<dyn Error>>;
    pub async fn post_users(&self, body: CreateUserRequest) -> Result<User, Box<dyn Error>>;
    
    // Methods with custom timeout
    pub async fn get_users_with_timeout(&self, timeout: Option<Duration>) -> Result<UsersResponse, Box<dyn Error>>;
}
```

### 4. OpenAPI Generation

The macro generates an OpenAPI 3.0 specification that can be used to generate TypeScript clients:

```rust
// Generated function to create OpenAPI spec
pub fn generate_userservice_openapi() -> serde_json::Value {
    // Returns the OpenAPI 3.0 JSON document
}

// Generated function to write OpenAPI spec to file
pub fn generate_userservice_openapi_to_file() -> std::io::Result<()> {
    // Writes to target/openapi/userservice.json
}
```

## TypeScript Client Usage

### 1. Generate OpenAPI Specification

Add a `build.rs` file to your backend crate to generate the OpenAPI spec at compile time:

```rust
// backend/build.rs
fn main() {
    // Import your API module
    use rest_api;
    
    // Generate OpenAPI spec to target directory
    rest_api::generate_userservice_openapi_to_file()
        .expect("Failed to generate OpenAPI spec");
}
```

This creates `target/openapi/userservice.json` during compilation.

### 2. TypeScript Usage

Generate a TypeScript fetch client from the OpenAPI document with your preferred
OpenAPI generator. The examples below assume the generated client exports
methods and schemas from `./generated`.

```typescript
import * as api from './generated';
import type { CreateUserRequest } from './generated';

// Shared configuration object for all requests
const baseConfig = {
  baseUrl: 'http://localhost:3000/api/v1',
  headers: {
    Authorization: 'Bearer admin-token'
  }
};

// Make API calls with named methods
const response = await api.getUsers(baseConfig);
if (response.data) {
  const users = response.data.users;
}

// GET with path parameter
const userResponse = await api.getUsersId(
  Object.assign({}, baseConfig, { path: { id: '123' } })
);

// POST with typed body
const newUser: CreateUserRequest = {
  name: 'John Doe',
  email: 'john@example.com'
};

const created = await api.postUsers(
  Object.assign({}, baseConfig, { body: newUser })
);

// DELETE request
await api.deleteUsersId(
  Object.assign({}, baseConfig, { path: { id: '123' } })
);
```

### Why Use An OpenAPI-Generated Fetch Client

- **Small browser surface**: Standard fetch client code instead of a full app scaffold
- **Better developer experience**: Standard TypeScript/JavaScript
- **Runtime flexibility**: Fetch-based clients can be used in common JavaScript runtimes and browsers
- **Tree-shaking friendly**: Standard JavaScript optimization applies
- **Easier Debugging**: Standard network requests in DevTools

## OpenAPI Documentation

### Enabling OpenAPI Generation

```rust
rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,                    // Generate to target/openapi/userservice.json
    serve_docs: true,                 // Enable the built-in API explorer
    docs_path: "/docs",               // API explorer path
    endpoints: [
        GET UNAUTHORIZED users() -> UsersResponse,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
    ]
});
```

Use `openapi: { output: "api.json" }` instead when you need a custom output path.

### Generated OpenAPI Features

- Endpoint documentation with request/response schemas
- Authentication requirements via `x-authentication` extension
- Permission requirements via `x-permissions` extension
- JSON Schema generation for all types
- Built-in API explorer integration

### Accessing OpenAPI Documentation

1. **API explorer**: Navigate to `http://localhost:3000/api/v1/docs`
2. **OpenAPI JSON**: Available at `http://localhost:3000/api/v1/docs/openapi.json`
3. **Generated File**: Check `target/openapi/<lowercase-service-name>.json` or custom path

## Error Handling

### Using RestResult and RestError

The macro uses `RestResult<T>` for all endpoints, allowing explicit HTTP status codes:

```rust
use ras_rest_core::{RestResult, RestResponse, RestError};

async fn get_user(&self, id: String) -> RestResult<User> {
    if id.trim().is_empty() {
        return Err(RestError::bad_request("Invalid user ID"));
    }

    let user = self
        .users
        .lock()
        .expect("users lock")
        .get(&id)
        .cloned()
        .ok_or_else(|| RestError::not_found("User not found"))?;

    Ok(RestResponse::ok(user))
}

async fn create_user(&self, request: CreateUserRequest) -> RestResult<User> {
    let user = User {
        id: "user-1".to_string(),
        name: request.name,
        email: request.email,
    };

    Ok(RestResponse::created(user))
}
```

### Client Error Handling

```typescript
try {
    const user = await client.get_users_by_id('invalid-id');
} catch (error) {
    // Error includes HTTP status and message
    console.error('Failed to get user:', error);
}
```

## Advanced Features

### 1. Usage Tracking

Track API usage for analytics or rate limiting:

```rust
.with_usage_tracker(|_headers, user, method, path| {
    let method = method.to_string();
    let path = path.to_string();
    let user_id = user.map(|user| user.user_id.clone());
    async move {
        println!("API call: {} {} by {:?}", method, path, user_id);
    }
})
```

### 2. Performance Monitoring

Track endpoint execution time:

```rust
.with_method_duration_tracker(|method, path, _user, duration| {
    let method = method.to_string();
    let path = path.to_string();
    async move {
        println!("{} {} took {:?}", method, path, duration);
    }
})
```

### 3. Complex Path Parameters

Support for multiple path parameters:

```rust
PUT WITH_PERMISSIONS(["user"]) 
    users/{user_id: String}/projects/{project_id: i32}/tasks/{task_id: Uuid}(UpdateTaskRequest) 
    -> Task,
```

### 4. Multiple Permission Groups

OR logic between groups, AND logic within:

```rust
// User needs either:
// - admin permission, OR
// - both moderator AND editor permissions, OR  
// - all three: viewer, commenter, and subscriber
WITH_PERMISSIONS(["admin"] | ["moderator", "editor"] | ["viewer", "commenter", "subscriber"])
```

## Task API Example

This example shows a task management API definition with public, authenticated,
and permission-gated routes:

```rust
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

// API Types
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateTaskRequest {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub completed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TasksResponse {
    pub tasks: Vec<Task>,
    pub total: usize,
}

// Define REST API
rest_service!({
    service_name: TaskService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    endpoints: [
        // List all tasks (public)
        GET UNAUTHORIZED tasks() -> TasksResponse,
        
        // Get specific task (public)
        GET UNAUTHORIZED tasks/{id: String}() -> Task,
        
        // Create task (requires authentication)
        POST WITH_PERMISSIONS(["user"]) tasks(CreateTaskRequest) -> Task,
        
        // Update task (owner or admin)
        PUT WITH_PERMISSIONS(["owner"] | ["admin"]) tasks/{id: String}(UpdateTaskRequest) -> Task,
        
        // Delete task (owner or admin)
        DELETE WITH_PERMISSIONS(["owner"] | ["admin"]) tasks/{id: String}() -> (),
        
        // Get user's tasks
        GET WITH_PERMISSIONS(["user"]) users/{user_id: String}/tasks() -> TasksResponse,
    ]
});

// TypeScript usage
/*
import * as api from './generated';

const userToken = 'user-token';
const baseConfig = {
  baseUrl: 'http://localhost:3000/api/v1',
  headers: { Authorization: `Bearer ${userToken}` }
};

// Create a task
const newTask = await api.postTasks(
  Object.assign({}, baseConfig, {
    body: {
      title: 'Complete documentation',
      description: 'Document REST endpoints'
    }
  })
);

// Update task
await api.putTasksId(
  Object.assign({}, baseConfig, {
    path: { id: newTask.data.id },
    body: { completed: true }
  })
);

// Get user's tasks
const myTasks = await api.getUsersUserIdTasks(
  Object.assign({}, baseConfig, { path: { user_id: userId } })
);
*/
```

## Best Practices

1. **Type Safety**: Always use strongly-typed request/response objects
2. **Error Handling**: Use appropriate HTTP status codes via `RestError`
3. **Authentication**: Implement proper bearer token validation in your `AuthProvider`
4. **Documentation**: Enable OpenAPI generation for API documentation
5. **Monitoring**: Use usage and duration trackers for observability
6. **CORS**: Configure CORS appropriately for frontend clients
7. **Validation**: Validate request data in your service implementation
8. **Logging**: Log internal errors while keeping client messages generic
9. **OpenAPI Output**: Use `build.rs` when you want to emit the OpenAPI spec at compile time
10. **Client Generation**: Generate clients from the OpenAPI document when you need frontend bindings

## Troubleshooting

### Common Issues

1. **Missing `JsonSchema` implementation**: All types must implement `JsonSchema` for OpenAPI generation
2. **OpenAPI generation fails**: Ensure `openapi: true` is set and all types implement `JsonSchema`
3. **TypeScript generation issues**: Verify the OpenAPI spec exists at the configured path
4. **Authentication fails**: Check that your `AuthProvider` is properly configured
5. **CORS errors**: Add appropriate CORS middleware to your Axum router

### Feature Flags

Control code generation with feature flags:

```toml
[features]
default = ["server"]
server = [
    "ras-rest-macro/server",
    "dep:ras-rest-core",
    "dep:ras-auth-core",
    "dep:async-trait",
    "dep:axum",
    "dep:axum-extra",
    "dep:tokio",
]
client = ["ras-rest-macro/client", "dep:reqwest"]
```

## Conclusion

The `ras-rest-macro` provides a typed workflow for building REST APIs in Rust with automatic client generation. By defining your API once, you get:

- Type-safe server implementation
- Native Rust client
- OpenAPI specification for TypeScript client generation
- Typed TypeScript clients when generated from the OpenAPI document
- Built-in authentication and authorization
- Performance monitoring and usage tracking

This approach avoids hand-maintained client DTOs and keeps browser clients aligned with the server contract. OpenAPI-based TypeScript generation also keeps the browser path easy to inspect and debug with standard network tooling.
