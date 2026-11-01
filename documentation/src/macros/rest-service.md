# `rest_service!`

`rest_service!` turns one service definition into a JSON REST API.
From that definition it generates a handler trait, axum routes that check auth and decode
path, query and body values, a typed Rust client, and, when you ask for it,
an OpenAPI 3.0 document and a hosted API explorer.

Use it for resource-style APIs with conventional HTTP methods and paths.
For command-style APIs, [`jsonrpc_service!`](jsonrpc-service.md) may fit better.

## Dependencies and features

Put the macro in a shared API crate,
so the server and its clients compile against the same definition.
The API crate exposes `server` and `client` features
that forward to the macro crate and enable the runtime crates the generated code refers to.

```toml
[dependencies]
ras-rest-macro = { version = "0.4", default-features = false }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"
async-trait = { version = "0.1", optional = true }
ras-transport-core = { version = "0.2", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-rest-core = { version = "0.2", optional = true }
ras-auth-core = { version = "0.3", optional = true }
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

A backend depends on the API crate with `features = ["server"]`;
a Rust client or WASM crate depends on the same crate with `features = ["client"]`.
The macro is a proc-macro crate,
so its own dependencies never reach your code;
that is why the API crate lists the runtime crates itself.
If one crate should always expose both sides, enable `server` and `client` directly
on the `ras-rest-macro` dependency and make the runtime dependencies non-optional.

The macro crate has these features:

| Feature | Default | Emits |
| --- | --- | --- |
| `server` | yes | The handler trait, the router builder and the OpenAPI functions. |
| `client` | through `reqwest` | The client and its builder, with `build_with_transport(...)`. |
| `reqwest` | yes | `client`, plus the reqwest-backed `build()` on the client builder. |
| `permissions` | no | A permission manifest; see [Permission manifests](../permission-manifests.md). |

The OpenAPI functions are part of the server code,
so a client-only build does not have them.
A crate that only injects its own transport forwards `ras-rest-macro/client`
and `dep:ras-transport-core` in place of `ras-rest-macro/reqwest`.

Cargo unifies the macro crate's features across a workspace,
so one crate that enables `client` turns client code on in every crate that calls the macro.
When that matters, set `feature_gated: true` in the service definition.
The macro then emits both sides and wraps them in `#[cfg(feature = "server")]`
and `#[cfg(feature = "client")]`, which your API crate's own features decide;
the client's `build()` sits behind `#[cfg(feature = "reqwest")]`.
Declare all three features in the API crate.

## Define the service

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
        /// List all users.
        GET UNAUTHORIZED users() -> Vec<User>,
        GET OPTIONAL_AUTH feed() -> Vec<User>,
        GET UNAUTHORIZED users/search ? name: String & limit: Option<u32> () -> Vec<User>,
        GET WITH_PERMISSIONS(["user"]) users/{id: String}() -> User,
        POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> User,
        DELETE WITH_PERMISSIONS(["admin"] | ["support", "users:delete"]) users/{id: String}() -> (),
    ]
});
```

The definition starts with `service_name` and `base_path`, in that order, and ends with `endpoints`.
Each endpoint is written as:

```text
/// Optional doc comment.
METHOD AUTH path/{param: Type}/segment ? query: Type & other: Type (RequestType) -> ResponseType { options },
```

* `METHOD` is `GET`, `POST`, `PUT`, `DELETE` or `PATCH`.
* `AUTH` is an auth requirement, described below.
* The path is a list of segments joined by `/`, relative to `base_path`.
  A plain segment is a Rust identifier, so it cannot contain `-`.
  `{name: Type}` is a path parameter; the first segment cannot be one.
* `? name: Type & other: Type` declares query parameters.
  An `Option<T>` parameter may be left out of the request,
  and a `Vec<T>` parameter reads a repeated key such as `?tag=a&tag=b`.
* `(RequestType)` is the JSON body type. Write `()` for an endpoint without a body.
* `ResponseType` is the JSON response type; use `()` for an empty one.

A doc comment becomes the operation's summary and description in the OpenAPI document;
other attributes are rejected.
Body, response and query types need `Serialize`, `Deserialize` and, with `openapi` on, `JsonSchema`.

`AUTH` is one of:

| Requirement | Meaning | Extra handler argument |
| --- | --- | --- |
| `UNAUTHORIZED` | Public. | none |
| `OPTIONAL_AUTH` | Public, and identifies the caller when a valid credential is present. | `caller: Caller` |
| `WITH_PERMISSIONS([...])` | Needs a valid credential and one matching permission group. | `user: &AuthenticatedUser` |

`WITH_PERMISSIONS(["a", "b"] | ["c"])` allows a caller with both `a` and `b`, or with `c`.
`WITH_PERMISSIONS([])` allows any authenticated caller.
An empty group mixed with other groups is a compile error,
because it would quietly let every authenticated caller through.
[Auth in the API contract](../auth-in-api-contract.md) explains the model.

### Handler names

The handler name, used by both the trait and the client,
is the HTTP method in lower case followed by the path segments,
with each path parameter written as `by_<name>`:

| Endpoint | Handler |
| --- | --- |
| `GET users()` | `get_users` |
| `GET users/search ? name: String ()` | `get_users_search` |
| `GET users/{id: String}()` | `get_users_by_id` |
| `POST users(CreateUserRequest)` | `post_users` |
| `DELETE users/{id: String}()` | `delete_users_by_id` |

## Request bodies and `Content-Type`

Endpoints that declare a body read and JSON-decode it *after* the
auth, CSRF and permission checks succeed,
so unauthenticated callers cannot make the server buffer or parse payloads.
By default a request whose `Content-Type` is not `application/json`
(parameters such as `; charset=utf-8` are allowed)
is rejected with `415 Unsupported Media Type` before the body is read.
A cross-origin `text/plain` POST skips the CORS preflight,
so requiring `application/json` closes that route for cross-site requests.

Malformed JSON is logged (category, line and column, never the value) and answered with `400`.
A body over the size limit is answered with `413`, and an unreadable stream with `400`.

To accept any content type, for example from a device client that cannot set the header,
opt out at the service level with `require_json_content_type: false`.

The check only applies to endpoints that declare a request body.
A mutating endpoint without a body, such as `POST logout() -> ()`, is not checked,
so its CSRF protection comes from the auth transport:
a bearer token is not sent automatically by the browser,
and cookie auth requires a CSRF header, which itself forces a preflight.

## Service and endpoint options

Service-level options go between `base_path` and `endpoints`, each followed by a comma:

| Option | Default | Meaning |
| --- | --- | --- |
| `openapi: true` | off | Generate the OpenAPI functions; `_to_file` writes `target/openapi/<service>.json`. |
| `openapi: { output: "path.json" }` | | The same, with your own output path for `_to_file`. |
| `serve_docs: <bool>` | `false` | Serve the API explorer and `openapi.json`. Needs `openapi`; `serve_docs: true` without it is a compile error that says so. |
| `docs_path: "..."` | `"/docs"` | Where the explorer is served, under `base_path`. The document is at `<docs_path>/openapi.json`. |
| `docs_require_auth: <bool>` | `false` | Require an authenticated caller for the explorer page and `openapi.json`. |
| `require_json_content_type: <bool>` | `true` | Reject a bodied request whose `Content-Type` is not `application/json` with 415. |
| `body_limit: <bytes>` | `2097152` (2 MiB) | Maximum request body size. |
| `feature_gated: <bool>` | `false` | Gate the server and client on the API crate's own features; see [Dependencies and features](#dependencies-and-features). |

> **Note:** when `serve_docs` is enabled the explorer page and `openapi.json` are
> public by default, and they show your operations, schemas and permission requirements.
> Set `docs_require_auth: true` to gate them, or disable `serve_docs` in production.
>
> `docs_require_auth` checks the credential the same way your endpoints do.
> A browser cannot send an `Authorization` header on a page navigation,
> so gated docs open in a browser only under cookie auth (`.auth_cookie(...)`).
> With bearer tokens only, they are reachable by a client that sets the header.

Per-endpoint options go in a trailing `{ ... }` block after the response type:

```rust,ignore
// 16 KiB cap and access to request headers for just this endpoint.
POST WITH_PERMISSIONS(["admin"]) devices/{id: String}(Telemetry) -> Ack {
    body_limit: 16384,
    headers: true,
}
```

| Option | Meaning |
| --- | --- |
| `body_limit: <bytes>` | Overrides the service body limit for this endpoint. |
| `headers: true` | Passes the request's `axum::http::HeaderMap` to the handler. |
| `version: label` | The current version label; see [Versioning](#versioning). |
| `versions: [...]` | Legacy routes; see [Versioning](#versioning). |
| `examples: [...]` | Request examples; see [Examples](#examples). |

With `headers: true`, the map arrives right after the caller or user argument,
before the path parameters.
It is how a handler reads a custom device header without a separate tower layer.
The map is *unredacted*: it still contains the caller's `Authorization`, `Cookie` and CSRF headers,
so do not log it or forward it upstream as it is;
`ras_auth_core::redact_sensitive_headers_for_auth_transport` removes them.

### Examples

A schema says what a request may contain; an example shows a request that works
and the response it gets.
Give an endpoint an `examples` list in its options block:

```rust,ignore
/// Rename an item.
POST WITH_PERMISSIONS(["items:write"]) items/{id: u32}/rename ? notify: Option<bool> (RenameItem) -> RenamedItem {
    examples: [
        /// Rename and notify watchers.
        notify_watchers {
            path: { id: 42 },
            query: { notify: Some(true) },
            body: RenameItem { name: "Spare parts".into() },
            response: RenamedItem { id: 42, name: "Spare parts".into() },
        },
        /// The item does not exist.
        missing_item {
            path: { id: 9999 },
            body: RenameItem { name: "Spare parts".into() },
            error: { status: 404, message: "Item not found" },
        },
    ],
}
```

The values are typed Rust expressions, so an example that does not fit its
endpoint fails to compile.

* Each example has a name, unique within the endpoint, and an optional doc comment that becomes its description.
* `path` gives every path parameter. `query` gives every query parameter that is
  not an `Option`; an `Option` one may be left out or set to `None`.
* `body` is required exactly when the endpoint takes a request body.
* A success has a `response` and an optional 2xx `status`, 200 by default.
  A `204` takes no `response`, and an endpoint returning `()` may leave it out.
* An error has `error: { status, message }`, a 4xx or 5xx status and the message
  your handler's `RestError` carries. It takes no `status` or `response` beside it.

Examples generate code only when `openapi` is on, and only in server builds.
Every example value must implement `Serialize`.
A request body must also read back unchanged:
it is serialized, deserialized as the request type and serialized again,
and the two JSON values must be equal.
Legacy versions take no examples.

### Versioning

An endpoint can serve older payload shapes at legacy paths
and migrate them to the current types, so your handler only ever sees the current shape.
Give the endpoint a `version` label and one or more `versions` entries:

```rust,ignore
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
```

Each entry needs a `path`, a `response` and a `migration`.
It may also give `query: [name: Type & other: Type]` and a body type, as `body` or `request`;
leave the body out, or write `()`, for a route without one.
Each legacy path becomes its own route with the endpoint's auth requirement.

The migration works on whole requests, not only bodies.
For each version the server generates a request struct named
`<Service><Handler><Version>Request`, with `path`, `query` and `body` fields,
and matching `...Path` and `...Query` structs.
The migration type implements `ras_rest_core::VersionMigration` twice:
from the legacy request struct to the current one, and from the current response to the legacy one.

```rust,ignore
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

The request structs exist only in server builds.
The client gets one method per legacy route, named from its path the same way as a handler,
here `post_v1_users_by_id_rename`.

## Implement the generated trait

The macro generates `UserServiceTrait`, an `async_trait` trait with one method per endpoint,
so the implementation needs `#[async_trait::async_trait]` too.
Each method returns `RestResult<T>`, an alias for `Result<RestResponse<T>, RestError>`.
Arguments come in this order: the caller or user, the headers when `headers: true`,
the path parameters, the query parameters, and the body as `request`.

```rust,ignore
use ras_auth_core::{AuthenticatedUser, Caller};
use ras_rest_core::{RestError, RestResponse, RestResult};

struct UserServiceImpl;

#[async_trait::async_trait]
impl UserServiceTrait for UserServiceImpl {
    async fn get_users(&self) -> RestResult<Vec<User>> {
        Ok(RestResponse::ok(vec![]))
    }

    async fn get_feed(&self, caller: Caller) -> RestResult<Vec<User>> {
        match caller.authenticated() {
            Some(user) => todo!("a feed for {}", user.user_id),
            None => Ok(RestResponse::ok(vec![])),
        }
    }

    async fn get_users_search(&self, name: String, limit: Option<u32>) -> RestResult<Vec<User>> {
        todo!("find up to {limit:?} users named {name}")
    }

    async fn get_users_by_id(&self, user: &AuthenticatedUser, id: String) -> RestResult<User> {
        Err(RestError::not_found(format!("no user {id}")))
    }

    async fn post_users(
        &self,
        user: &AuthenticatedUser,
        request: CreateUserRequest,
    ) -> RestResult<User> {
        Ok(RestResponse::created(User {
            id: "u-1".to_string(),
            name: request.name,
        }))
    }

    async fn delete_users_by_id(&self, user: &AuthenticatedUser, id: String) -> RestResult<()> {
        Ok(RestResponse::no_content())
    }
}
```

`RestResponse` has `ok`, `created`, `accepted`, `no_content` and `with_status`.
A `204` response is sent without a body.
`RestError` has helpers such as `bad_request`, `not_found` and `conflict`;
`RestError::with_internal(status, message, error)` logs `error` server-side and sends only `message`.

Every error the client sees has the body `{ "error": "<message>" }`,
whether it comes from your handler or from the framework.
A missing or invalid credential on a `WITH_PERMISSIONS` endpoint is answered with 401,
and a failed permission or CSRF check with 403.
`UNAUTHORIZED` and `OPTIONAL_AUTH` endpoints ignore a credential that fails.

## Build the router

```rust,ignore
let app = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
    .build();
```

`build()` returns an `axum::Router` with every route nested under `base_path`,
ready to merge into your application.
It panics when the service has a `WITH_PERMISSIONS` endpoint, or gated docs,
and no `.auth_provider(...)` was set,
because every such request would otherwise fail at runtime.
It also panics when the auth transport configuration is invalid.

The builder methods are:

| Method | Purpose |
| --- | --- |
| `new(service)` | Wraps your trait implementation. |
| `auth_provider(provider)` | The `AuthProvider` that turns a credential into an `AuthenticatedUser`. |
| `auth_cookie(config)` | Accept a session cookie as well as a bearer token, with double-submit CSRF protection. |
| `csrf_protection(config)` | Replace the CSRF configuration. |
| `auth_transport(config)` | Replace the whole credential transport configuration. |
| `with_usage_tracker(f)` | Async hook called once authorization passes, before the body is decoded, with the headers (credentials redacted), the user, the HTTP method and the route path. |
| `with_method_duration_tracker(f)` | Async hook called once the response is built, with the HTTP method, the route path, the user, the duration and whether the request succeeded. |

A request rejected by authentication or authorization reaches neither hook.
Every other request reaches both, once each.
`success` is `false` when the response status is 4xx or 5xx,
such as a body that does not decode or a handler error.
[Observability](../observability.md) shows how to feed both hooks into OpenTelemetry.

Cookie auth works with the same `AuthProvider`:

```rust,ignore
use ras_auth_core::{AuthCookieConfig, CsrfConfig};

let app = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
    .auth_cookie(AuthCookieConfig::default())
    .csrf_protection(CsrfConfig::default())
    .build();
```

Bearer tokens stay accepted, and a bearer token wins when both are present.
The CSRF check applies only to cookie-authenticated `POST`, `PUT`, `PATCH` and `DELETE` requests.
`CsrfConfig::default()` requires an `x-ras-csrf` header that matches the `__Host-ras-csrf` cookie.
[Auth in the API contract](../auth-in-api-contract.md) shows how to write an `AuthProvider`.

## Use the generated Rust client

Enable the shared API crate's `client` feature in the crate that makes outbound calls:

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
let found = client.get_users_search("Alice".to_string(), Some(10)).await?;
let alice = client.get_users_by_id("alice".to_string()).await?;

client.set_bearer_token(Some(admin_token));

let created = client
    .post_users(CreateUserRequest {
        name: "Alice".to_string(),
    })
    .await?;

client.delete_users_by_id(created.id).await?;
```

Client methods have the handler names.
Path parameters, query parameters and the request body become ordinary arguments, in that order,
and every call returns `Result<T, ras_transport_core::TransportError>`.
Each method also has a `<handler>_with_timeout(..., timeout)` variant.
`build()` uses the reqwest transport;
`build_with_transport(transport)` takes any `Arc<dyn HttpTransport>`.

## Use an OpenAPI TypeScript client

A browser client can be generated from the OpenAPI document instead.
The REST example generates a fetch client and calls named functions with `baseUrl`,
optional headers, path parameters, query parameters and body values.
The function names come from each operation's `operationId`,
the HTTP method followed by the path segments, such as `get_users_id`.

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

## OpenAPI, explorer, and clients

With `openapi: true`, server builds get:

```rust,ignore
pub fn generate_userservice_openapi() -> serde_json::Value;
pub fn try_generate_userservice_openapi()
    -> Result<serde_json::Value, ras_rest_core::SpecGenerationError>;
pub fn generate_userservice_openapi_to_file() -> std::io::Result<()>;
```

The function names use the service name in lower case.
`try_generate_*` names the endpoint, example and part that could not be built;
`generate_*` panics with that error and `_to_file` returns it as an `io::Error`.
`_to_file` writes to the `openapi` output path, relative to the working directory.

With `serve_docs: true`, the generated router serves the built-in API explorer
at `docs_path` under `base_path`. The served `openapi.json` is generated
once per service and kept; a failure answers 500 and is logged.

The OpenAPI document includes JSON schemas, routes, HTTP methods, bearer auth
requirements, `x-permissions` and `x-permission-groups` metadata, and examples.
A versioned endpoint carries `x-ras-version`,
and each legacy route adds `x-ras-canonical-version` and `x-ras-canonical-path`.
The document validates against the OpenAPI 3.0 schema, and it can be checked into build output or consumed by
TypeScript client generators.

OpenAPI 3.0 has no request/response pairs, so an example appears under its name
in every place it touches: its parameters, its request body and the response for
its status. A status that no response listed yet gets one.

Every error response is described by the `RestErrorBody` component,
`{ "error": string }`. That is the body of a handler's `RestError` and of every
request the framework rejects, from a path parameter that does not parse to a
missing credential. `RestErrorBody` is a reserved component name, and every
component name may only use letters, digits, `.`, `-` and `_`; a type whose schema
breaks either rule fails generation. A 204 example has no content to sit on and is
recorded under `x-ras-examples` on the 204 response.

## Generated items

For a service named `UserService`:

| Item | When |
| --- | --- |
| `UserServiceTrait`, `UserServiceBuilder` | server code |
| `<Service><Handler><Version>Request`, `...Path`, `...Query` | server code, for endpoints with `versions` |
| `generate_userservice_openapi`, `try_generate_userservice_openapi`, `generate_userservice_openapi_to_file` | server code with `openapi` on |
| `UserServiceClient`, `UserServiceClientBuilder` | client code |
| `generate_userservice_permission_manifest()`, module `userservice_permissions` | the macro crate's `permissions` feature |

See
[examples/rest-wasm-example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/rest-wasm-example)
for a REST API with OpenAPI output and browser client usage.
