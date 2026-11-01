# ras-rest-macro

The `rest_service!` macro: one definition of a JSON REST API,
from which it generates axum routes, a typed Rust client and an OpenAPI 3.0 document.

A hand-written REST API keeps its contract in several places at once.
The routes, the auth checks, the client and the OpenAPI file each repeat the paths and types,
and nothing stops them from drifting apart.

`rest_service!` keeps the contract in one place.
Each endpoint declares its HTTP method, auth requirement, path, body type and response type,
and the macro generates the rest from that declaration.

```rust
use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct User {
    pub id: i32,
    pub name: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CreateUserRequest {
    pub name: String,
}

rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,
    endpoints: [
        GET UNAUTHORIZED users() -> Vec<User>,
        GET WITH_PERMISSIONS(["user"]) users/{id: i32}() -> User,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
    ]
});
```

From this the macro generates:

* `UserServiceTrait`, with one async method per endpoint for you to implement:
  `get_users`, `get_users_by_id` and `post_users`.
  Protected endpoints receive the `&AuthenticatedUser` that passed the permission check.
* `UserServiceBuilder`, which turns your implementation and an `AuthProvider` into an `axum::Router`
  nested under `base_path`.
* `UserServiceClient`, with typed methods of the same names.
* `generate_userservice_openapi()` and its siblings, which build the OpenAPI document.
  `serve_docs: true` also serves an API explorer.

Serving the service takes a trait implementation and a builder call:

```rust,ignore
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestError, RestResponse, RestResult};

struct Users;

#[async_trait::async_trait]
impl UserServiceTrait for Users {
    async fn get_users(&self) -> RestResult<Vec<User>> {
        Ok(RestResponse::ok(vec![]))
    }

    async fn get_users_by_id(&self, _user: &AuthenticatedUser, id: i32) -> RestResult<User> {
        Err(RestError::not_found(format!("no user {id}")))
    }

    async fn post_users(
        &self,
        _user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> RestResult<User> {
        Ok(RestResponse::created(User { id: 1, name: request.name }))
    }
}

let app = UserServiceBuilder::new(Users)
    .auth_provider(my_auth_provider)
    .build();
```

Handlers choose the status code through `RestResponse` and `RestError`.
The client sees an error as `{ "error": "<message>" }`.

## Installation

The generated code refers to runtime crates by name,
so a crate that calls the macro depends on them as well.
For one crate with both the server and the client:

```toml
[dependencies]
ras-rest-macro = "0.4"
ras-rest-core = "0.2"
ras-auth-core = "0.3"
ras-transport-core = { version = "0.2", features = ["reqwest"] }
async-trait = "0.1"
axum = "0.8"
axum-extra = { version = "0.10", features = ["query"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"
```

The macro crate's features choose what it generates:

| Feature | Default | Emits |
| --- | --- | --- |
| `server` | yes | The handler trait, the router builder and the OpenAPI functions. |
| `client` | through `reqwest` | The client, with `build_with_transport(...)`. |
| `reqwest` | yes | `client`, plus the reqwest-backed `build()`. |
| `permissions` | no | A permission manifest, using `ras-permission-manifest`. |

Most projects put the definition in a shared API crate,
with `server` and `client` features that the backend and the clients pick from.
The guide shows that layout, and the client compiles to WASM.

## Documentation

The [`rest_service!` guide](https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/macros/rest-service.md)
is the full reference: endpoint syntax with path and query parameters, service and endpoint options,
auth requirements, request examples in the OpenAPI document, versioned endpoints,
cookie auth and the generated client.

Runnable code is in
[examples/rest-wasm-example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/rest-wasm-example).

## License

Licensed under either MIT or Apache-2.0, at your option.
