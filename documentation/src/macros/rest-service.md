# `rest_service!`

Use `rest_service!` for JSON REST APIs that should generate Axum routes, typed
handler traits, native Rust clients, OpenAPI documents, and an optional API
explorer.

## Dependencies And Features

```toml
[dependencies]
ras-rest-macro = { version = "0.3.0", default-features = false }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.0.0-alpha.20"
async-trait = { version = "0.1", optional = true }
ras-transport-core = { version = "0.1.0", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-rest-core = { version = "0.2.0", optional = true }
ras-auth-core = { version = "0.2.0", optional = true }
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

## Request Bodies And `Content-Type`

Endpoints that declare a body read and JSON-decode it **after** the
auth/CSRF/permission checks succeed, so unauthenticated callers cannot make the
server buffer or parse payloads. By default a request whose `Content-Type` is
not `application/json` (parameters such as `; charset=utf-8` are allowed) is
rejected with `415 Unsupported Media Type` before the body is read. Requiring
`application/json` forces a CORS preflight for cross-origin requests, closing the
simple-request CSRF shape (a cross-origin `text/plain` POST). Malformed JSON is
logged (category + line/column, never the value) and answered with `400`; a body
over the limit is `413`, distinct from an unreadable stream (`400`).

To accept any content type — for example a device client that cannot set the
header — opt out at the service level with `require_json_content_type: false`.

The gate only applies to endpoints that declare a request body. A bodiless
mutating endpoint (e.g. `POST logout() -> ()`) has no body to type-check and is
not gated, so its CSRF protection comes from the auth transport: a bearer token
is not ambient, and cookie auth carries a mandatory CSRF header (a non-safelisted
header that itself forces a preflight).

## Service And Endpoint Options

Service-level options (alongside `service_name` / `base_path` / `endpoints`):

| Option | Default | Meaning |
| --- | --- | --- |
| `body_limit: <bytes>` | `2 * 1024 * 1024` | Maximum request body size. |
| `require_json_content_type: <bool>` | `true` | Enforce `application/json` on bodied endpoints. |
| `serve_docs: <bool>` / `docs_path: "..."` | `false` / `/docs` | Host the API explorer and `openapi.json`. |
| `docs_require_auth: <bool>` | `false` | Gate the docs page and `openapi.json` behind authentication (any authenticated user). |
| `feature_gated: <bool>` | `false` | Wrap the server/client in the consumer crate's own `server`/`client` features. |

> **Note:** when `serve_docs` is enabled the docs page and `openapi.json` are
> served **without** authentication by default, exposing your method names,
> schemas, and permission requirements. Set `docs_require_auth: true` to gate
> them, or disable `serve_docs` in production.
>
> `docs_require_auth` gates the whole explorer (page + spec) with the same
> credential check as your endpoints. Because a browser top-level navigation
> cannot send an `Authorization` header, the gated docs are only reachable in a
> browser under **cookie** auth (`.auth_cookie(...)`); on a bearer-only transport
> they are reachable only by a programmatic client that sets the header. Use it
> when the docs live behind cookie auth or should be hidden from browsers
> entirely.

Per-endpoint options go in a trailing `{ ... }` block after the response type:

```rust,ignore
// 16 KiB cap and access to request headers for just this endpoint.
POST WITH_PERMISSIONS(["admin"]) devices/{id: String}(Telemetry) -> Ack {
    body_limit: 16384,
    headers: true,
}
```

* `body_limit: <bytes>` overrides the service body limit for this endpoint.
* `headers: true` passes the request `axum::http::HeaderMap` to the handler as an
  extra argument, immediately after the caller/user and before the path
  parameters — the way to read a custom device header or the credential presence
  without a separate tower layer. The map is **unredacted**: it still contains the
  caller's `Authorization`, `Cookie`, and CSRF headers, so do not log it or
  forward it upstream verbatim (use
  `ras_auth_core::redact_sensitive_headers_for_auth_transport` if you need to).

### Versioning

An endpoint can serve older payload shapes at legacy paths and migrate them to
the canonical types. Give the endpoint a `version:` label and one or more
`versions:` entries, each with its own `path`, `request`, `response`, and a
`migration:` type implementing `ras_rest_core::VersionMigration` for both the
request (legacy → canonical) and response (canonical → legacy):

```rust,ignore
POST WITH_PERMISSIONS(["admin"]) items/{id: String}(RenameItemV2) -> RenamedItemV2 {
    version: "v2",
    versions: [
        "v1" {
            path: items/{id: String}/rename,
            request: RenameItemV1,
            response: RenamedItemV1,
            migration: RenameMigration,
        },
    ],
}
```

Each legacy path becomes its own route sharing the endpoint's auth level.

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
