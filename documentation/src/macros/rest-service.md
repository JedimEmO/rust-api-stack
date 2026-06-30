# `rest_service!`

Use `rest_service!` for JSON REST APIs that should generate Axum routes, typed
handler traits, native Rust clients, OpenAPI documents, and an optional API
explorer.

## Dependencies And Features

```toml
[dependencies]
ras-rest-macro = { version = "0.2.1", default-features = false }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.0.0-alpha.20"
async-trait = { version = "0.1", optional = true }
ras-transport-core = { version = "0.1.0", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-rest-core = { version = "0.1.1", optional = true }
ras-auth-core = { version = "0.1.0", optional = true }
axum = { version = "0.8", optional = true }
axum-extra = { version = "0.10", features = ["query"], optional = true }
tokio = { version = "1.0", features = ["full"], optional = true }

[features]
default = []
server = [
    "ras-rest-macro/server",
    "dep:ras-rest-core",
    "dep:ras-auth-core",
    "dep:async-trait",
    "dep:axum",
    "dep:axum-extra",
    "dep:tokio",
]
client = ["ras-rest-macro/reqwest", "ras-transport-core/reqwest"]
```

These API-crate features are forwarding gates. They enable the relevant macro
crate feature and the runtime dependencies that generated code refers to. The
macro emits server or client code only when the corresponding
`ras-rest-macro` feature is enabled; the generated code does not depend on a
consumer-crate `#[cfg(feature = "...")]` branch.

A backend depends on the API crate with `features = ["server"]`; a Rust client
or WASM crate depends on the same crate with `features = ["client"]`. If one
crate should always expose both surfaces, enable `server` and `client` directly
on the `ras-rest-macro` dependency and make the runtime dependencies non-optional.

The macro crate's `client` feature emits the generated client types and
`build_with_transport(...)`. Its `reqwest` feature also emits the default
reqwest-backed `build()`. If a crate only injects a custom transport, forward
`ras-rest-macro/client` plus `dep:ras-transport-core` instead of
`ras-rest-macro/reqwest`.

## Define The Service

```rust,ignore
use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateUserRequest {
    pub name: String,
}

rest_service!({
    service_name: UserService,
    base_path: "/api/v1",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    endpoints: [
        GET UNAUTHORIZED users() -> Vec<User>,
        GET OPTIONAL_AUTH feed() -> Vec<User>,
        GET WITH_PERMISSIONS(["user"]) users/{id: String}() -> User,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
        DELETE WITH_PERMISSIONS(["admin"] | ["support", "users:delete"]) users/{id: String}() -> (),
    ]
});
```

Endpoint syntax is:

```text
METHOD AUTH_REQUIREMENT path/{param: Type}/segments(RequestType) -> ResponseType
```

Supported methods are `GET`, `POST`, `PUT`, `DELETE`, and `PATCH`.
`AUTH_REQUIREMENT` is one of `UNAUTHORIZED`, `OPTIONAL_AUTH`, or
`WITH_PERMISSIONS([...])` — see
[Auth In The API Contract](../auth-in-api-contract.md). An `OPTIONAL_AUTH`
handler receives a `ras_auth_core::Caller` as its first argument: the route is
public, but identifies the caller when a valid credential is present.

## Implement The Generated Trait

REST handlers return `RestResult<T>`, usually through `RestResponse` helpers:

```rust,ignore
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestError, RestResponse, RestResult};

struct UserServiceImpl;

#[async_trait::async_trait]
impl UserServiceTrait for UserServiceImpl {
    async fn get_users(&self) -> RestResult<Vec<User>> {
        Ok(RestResponse::ok(vec![]))
    }

    async fn get_users_by_id(
        &self,
        user: &AuthenticatedUser,
        id: String,
    ) -> RestResult<User> {
        todo!("load a user visible to user.user_id")
    }

    async fn post_users(
        &self,
        user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> RestResult<User> {
        todo!("create user as admin")
    }
}
```

Path parameters become ordinary typed arguments. Protected endpoints receive
`&AuthenticatedUser` before path and body arguments.

## Build The Router

```rust,ignore
let app = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
    .build();
```

The builder can also be configured for secure cookie auth and CSRF protection
without changing the `AuthProvider`.

## Use The Generated Rust Client

Enable the shared API crate's `client` feature in the crate that makes outbound
calls:

```toml
[dependencies]
my-rest-api = { path = "../rest-api", default-features = false, features = ["client"] }
```

Pass the server origin to the generated client; the macro's `base_path` is
joined automatically.

```rust,ignore
let mut client = UserServiceClient::builder("http://localhost:3000")
    .with_timeout(std::time::Duration::from_secs(10))
    .build()?;

let users = client.get_users().await?;
let alice = client.get_users_by_id("alice".to_string()).await?;

client.set_bearer_token(Some(admin_token));

let created = client
    .post_users(CreateUserRequest {
        name: "Alice".to_string(),
    })
    .await?;

client.delete_users_by_id(created.id).await?;
```

Path parameters, query parameters, and request bodies become ordinary method
arguments in that order.

## Use An OpenAPI TypeScript Client

The REST examples also show the browser-oriented path: generate a fetch client
from the OpenAPI document, then call named functions with `baseUrl`, optional
headers, path parameters, query parameters, and body values.

```typescript
import { getUsers, getUsersId, postUsers } from './generated';
import type { CreateUserRequest } from './generated';

const baseUrl = 'http://localhost:3000/api/v1';

const users = await getUsers({ baseUrl });

const alice = await getUsersId({
  baseUrl,
  path: { id: 'alice' },
});

const request: CreateUserRequest = { name: 'Alice' };

const created = await postUsers({
  baseUrl,
  headers: { Authorization: `Bearer ${adminToken}` },
  body: request,
});
```

## OpenAPI, Explorer, And Clients

With `openapi: true`, the macro generates:

```rust,ignore
pub fn generate_userservice_openapi() -> serde_json::Value;
pub fn generate_userservice_openapi_to_file() -> std::io::Result<()>;
```

With `serve_docs: true`, the generated router serves the built-in API explorer
under `docs_path` relative to `base_path`.

The OpenAPI document includes JSON schemas, routes, HTTP methods, bearer auth
requirements, and `x-permissions` metadata. It can be checked into build output
or consumed by TypeScript client generators.

See
[examples/rest-wasm-example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/rest-wasm-example)
for a REST API with OpenAPI output and browser client usage.
