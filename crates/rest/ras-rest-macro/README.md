# ras-rest-macro

A procedural macro for creating type-safe REST APIs with authentication integration and OpenAPI document generation.

See the canonical mdBook
[`rest_service!` guide](../../../documentation/src/macros/rest-service.md) for
the rationale, auth model, usage flow, and runnable examples.

## Features

- **Type-safe REST endpoints**: Generate axum-based REST services from macro definitions
- **Authentication integration**: Seamless integration with `ras-auth-core::AuthProvider`
- **Permission-based access control**: Support for role-based authorization
- **Versioned endpoints**: Optional request/response migrations for legacy routes
- **OpenAPI 3.0 generation**: Automatic OpenAPI documentation using schemars
- **HTTP methods**: Support for GET, POST, PUT, DELETE, PATCH
- **Path parameters**: Type-safe path parameter extraction
- **Request/Response bodies**: JSON request and response handling

## Usage

### Basic Example

```rust
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, JsonSchema)]
struct User {
    id: i32,
    name: String,
    email: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct CreateUserRequest {
    name: String,
    email: String,
}

rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,
    endpoints: [
        GET UNAUTHORIZED users() -> Vec<User>,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
        GET WITH_PERMISSIONS(["user"]) users/{id: i32}() -> User,
        PUT WITH_PERMISSIONS(["admin"]) users/{id: i32}(CreateUserRequest) -> User,
        DELETE WITH_PERMISSIONS(["admin"]) users/{id: i32}() -> (),
    ]
});

// The macro generates:
// - UserServiceTrait: A trait with async methods for each endpoint
// - UserServiceBuilder: A builder for configuring the service implementation and auth provider
```

### Service Configuration

```rust
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestResponse, RestResult};

struct UserServiceImpl;

#[async_trait::async_trait]
impl UserServiceTrait for UserServiceImpl {
    async fn get_users(&self) -> RestResult<Vec<User>> {
        Ok(RestResponse::ok(vec![]))
    }

    async fn post_users(
        &self,
        _user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> RestResult<User> {
        Ok(RestResponse::created(User {
            id: 1,
            name: request.name,
            email: request.email,
        }))
    }

    async fn get_users_by_id(&self, _user: &AuthenticatedUser, id: i32) -> RestResult<User> {
        Ok(RestResponse::ok(User {
            id,
            name: "John".to_string(),
            email: "john@example.com".to_string(),
        }))
    }

    async fn put_users_by_id(
        &self,
        _user: &AuthenticatedUser,
        id: i32,
        request: CreateUserRequest,
    ) -> RestResult<User> {
        Ok(RestResponse::ok(User {
            id,
            name: request.name,
            email: request.email,
        }))
    }

    async fn delete_users_by_id(&self, _user: &AuthenticatedUser, _id: i32) -> RestResult<()> {
        Ok(RestResponse::no_content())
    }
}

let service = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
    .build();

// Use with axum
let app = axum::Router::new().merge(service);
```

Cookie auth can be enabled on the same service without changing the
`AuthProvider`:

```rust
use ras_auth_core::{AuthCookieConfig, CsrfConfig};

let service = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
    .auth_cookie(AuthCookieConfig::default())
    .csrf_protection(CsrfConfig::default())
    .build();
```

Bearer tokens remain accepted by default. If both bearer and cookie credentials
are present, bearer takes precedence. The CSRF guard only applies to
cookie-authenticated `POST`, `PUT`, `PATCH`, and `DELETE` requests.
`CsrfConfig::default()` requires the `x-ras-csrf` header to match the
`__Host-ras-csrf` double-submit cookie.

### OpenAPI Generation

```rust
// Generate OpenAPI document programmatically
let openapi_doc = generate_userservice_openapi();

// Write to file
generate_userservice_openapi_to_file().unwrap();
```

## Macro Syntax

### Service Definition

```rust
rest_service!({
    service_name: ServiceName,           // Name for the generated trait and builder
    base_path: "/api/v1",               // Base path for all endpoints
    openapi: true,                      // Enable OpenAPI generation (optional)
    endpoints: [
        GET UNAUTHORIZED users() -> Vec<User>,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
    ]
});
```

Use `openapi: { output: "target/openapi/service.json" }` instead of
`openapi: true` when you want a custom output path.

### Endpoint Definition

```rust
METHOD AUTH_REQUIREMENT path(RequestType) -> ResponseType,
```

- **METHOD**: `GET`, `POST`, `PUT`, `DELETE`, or `PATCH`
- **AUTH_REQUIREMENT**: 
  - `UNAUTHORIZED`: No authentication required
  - `WITH_PERMISSIONS(["perm1", "perm2"])`: Requires authentication and all listed permissions
  - `WITH_PERMISSIONS(["admin"] | ["moderator", "editor"])`: Allows any matching permission group
- **path**: URL path with optional parameters in `{param: Type}` format
- **RequestType**: Optional request body type (omit `()` for no body)
- **ResponseType**: Response type

### Examples

```rust
// Simple GET endpoint with no auth
GET UNAUTHORIZED users() -> Vec<User>,

// POST endpoint requiring admin permission with request body
POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,

// GET endpoint with path parameter requiring user permission
GET WITH_PERMISSIONS(["user"]) users/{id: i32}() -> User,

// Multiple path parameters
GET UNAUTHORIZED posts/{user_id: i32}/comments/{comment_id: String}() -> Comment,
```

### Versioned Endpoints

Versioning is opt-in. The canonical endpoint is handled by the generated trait method, and each legacy route is migrated into the canonical request parts before the service implementation is called.

```rust
#[derive(Serialize, Deserialize, JsonSchema)]
struct RenameUserV1 {
    name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct RenameUserV2 {
    display_name: String,
    notify: bool,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct RenameUserResponseV1 {
    name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct RenameUserResponseV2 {
    display_name: String,
    notified: bool,
}

rest_service!({
    service_name: UserService,
    base_path: "/api",
    endpoints: [
        POST UNAUTHORIZED v2/users/{id: i32}/rename(RenameUserV2) -> RenameUserResponseV2 {
            version: v2,
            versions: [
                v1 {
                    path: v1/users/{id: i32}/rename,
                    body: RenameUserV1,
                    response: RenameUserResponseV1,
                    migration: RenameUserCompat,
                },
            ],
        },
    ]
});

struct RenameUserCompat;

impl ras_rest_core::VersionMigration<
    UserServicePostV2UsersByIdRenameV1Request,
    UserServicePostV2UsersByIdRenameV2Request,
> for RenameUserCompat {
    type Error = std::convert::Infallible;

    fn migrate(
        value: UserServicePostV2UsersByIdRenameV1Request,
    ) -> Result<UserServicePostV2UsersByIdRenameV2Request, Self::Error> {
        Ok(UserServicePostV2UsersByIdRenameV2Request {
            path: UserServicePostV2UsersByIdRenameV2Path { id: value.path.id },
            query: UserServicePostV2UsersByIdRenameV2Query {},
            body: RenameUserV2 {
                display_name: value.body.name,
                notify: false,
            },
        })
    }
}

impl ras_rest_core::VersionMigration<RenameUserResponseV2, RenameUserResponseV1>
    for RenameUserCompat
{
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserResponseV2) -> Result<RenameUserResponseV1, Self::Error> {
        Ok(RenameUserResponseV1 {
            name: value.display_name,
        })
    }
}
```

## Authentication Integration

The macro integrates with `ras-auth-core::AuthProvider` for authentication:

```rust
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::HashSet;

struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
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

let service = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(MyAuthProvider)
    .build();
```

## Requirements

All request and response types must implement:
- `serde::Serialize` + `serde::Deserialize`
- `schemars::JsonSchema` (for OpenAPI generation)

```rust
#[derive(Serialize, Deserialize, JsonSchema)]
struct MyType {
    field: String,
}
```

## Generated Code

The macro generates:
1. **Service Trait**: `{ServiceName}Trait` with async methods for each endpoint
2. **Builder**: `{ServiceName}Builder` for configuration
3. **OpenAPI Functions**: `generate_{servicename}_openapi()` and `generate_{servicename}_openapi_to_file()`

## Integration with Axum

The generated service returns an `axum::Router` that can be used directly or merged with other routers:

```rust
let app = axum::Router::new()
    .merge(user_service)
    .merge(other_service);
```

## OpenAPI 3.0 Support

- Automatic schema generation from Rust types using schemars
- Authentication requirements in OpenAPI security schemes
- Permission metadata as OpenAPI extensions
- Path parameters and request/response schemas
- Standard HTTP error responses

## Checks

```bash
cargo test -p ras-rest-macro --locked
cargo clippy -p ras-rest-macro --all-targets --all-features --locked -- -D warnings
```
