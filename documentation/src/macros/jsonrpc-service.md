# `jsonrpc_service!`

`jsonrpc_service!` turns one service definition into a JSON-RPC 2.0 API over HTTP POST.
From that definition it generates a handler trait, an axum router that checks auth and decodes params,
a typed Rust client, and, when you ask for it, an OpenRPC document and a hosted API explorer.

Use it for command-style APIs, where a call is a named operation rather than a resource.
For resource-style JSON APIs, [`rest_service!`](rest-service.md) is the better fit.

## Dependencies and features

Put the macro in a shared API crate,
so the server and its clients compile against the same definition.
The API crate exposes `server` and `client` features
that forward to the macro crate and enable the runtime crates the generated code refers to.

```toml
[dependencies]
ras-jsonrpc-macro = { version = "0.4", default-features = false }
ras-jsonrpc-types = "0.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"
ras-transport-core = { version = "0.2", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-jsonrpc-core = { version = "0.3", optional = true }
axum = { version = "0.8", optional = true }
tokio = { version = "1.0", features = ["full"], optional = true }

[features]
default = []
server = ["ras-jsonrpc-macro/server", "dep:ras-jsonrpc-core", "dep:axum", "dep:tokio"]
client = ["ras-jsonrpc-macro/reqwest", "ras-transport-core/reqwest"]
```

Server binaries then depend on the API crate with `features = ["server"]`,
and clients depend on it with `features = ["client"]`.
The macro is a proc-macro crate,
so its own dependencies never reach your code;
that is why the API crate lists `ras-jsonrpc-core`, `axum` and `ras-transport-core` itself.

The macro crate has these features:

| Feature | Default | Emits |
| --- | --- | --- |
| `server` | yes | The handler trait and the router builder. |
| `client` | through `reqwest` | The client and its builder, with `build_with_transport(...)`. |
| `reqwest` | yes | `client`, plus the reqwest-backed `build()` on the client builder. |
| `permissions` | no | A permission manifest; see [Permission manifests](../permission-manifests.md). |

The OpenRPC functions are emitted in every build when `openrpc` is on,
client-only and wasm builds included.
A crate that only injects its own transport forwards `ras-jsonrpc-macro/client`
and `dep:ras-transport-core` in place of `ras-jsonrpc-macro/reqwest`.

Cargo unifies the macro crate's features across a workspace,
so one crate that enables `client` turns client code on in every crate that calls the macro.
When that matters, set `feature_gated: true` in the service definition.
The macro then emits both sides and wraps them in `#[cfg(feature = "server")]`
and `#[cfg(feature = "client")]`, which your API crate's own features decide;
the client's `build()` sits behind `#[cfg(feature = "reqwest")]`.
Declare all three features in the API crate.

## Define the service

```rust,ignore
use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SignInRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SignInResponse {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Feed {
    pub items: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct UserProfile {
    pub user_id: String,
}

jsonrpc_service!({
    service_name: UserService,
    openrpc: true,
    methods: [
        /// Exchange credentials for a bearer token.
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        OPTIONAL_AUTH feed(()) -> Feed,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS(["admin"] | ["support", "users:write"]) disable_user(String) -> (),
    ]
});
```

Each method is written as:

```text
/// Optional doc comment.
AUTH method_name(RequestType) -> ResponseType { options },
```

The Rust method name is also the JSON-RPC wire method, unless the options block sets `wire`.
The request and response types need `Serialize`, `Deserialize` and, with `openrpc` on, `JsonSchema`.
Use `()` for a method that takes no params.
A doc comment becomes the method's summary and description in the OpenRPC document;
other attributes are rejected.

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

The options block is optional and takes these fields:

| Field | Meaning |
| --- | --- |
| `wire: "name"` | The wire method name, when it differs from the Rust name. |
| `version: label` | The current version label, an identifier such as `v2` or a string such as `"2.0"`. |
| `versions: [...]` | Legacy wire methods; see [Versioned methods](#versioned-methods). |
| `examples: [...]` | Call examples; see [Show calls with examples](#show-calls-with-examples). |

## Service options

The definition starts with `service_name` and ends with `methods`.
These optional fields go between them:

| Option | Default | Meaning |
| --- | --- | --- |
| `openrpc: true` | off | Generate the OpenRPC functions; `_to_file` writes `target/openrpc/<service>.json`. |
| `openrpc: { output: "path.json" }` | | The same, with your own output path for `_to_file`. |
| `explorer: true` | off | Serve the API explorer at `<base_url>/explorer` and the document at `<base_url>/explorer/openrpc.json`. Needs `openrpc`; `explorer` without it is a compile error that says so. |
| `explorer: { path: "/docs" }` | | The same, at your own path under `base_url`. |
| `docs_require_auth: <bool>` | `false` | Require an authenticated caller for the explorer page and `openrpc.json`. |
| `require_json_content_type: <bool>` | `true` | Reject a request whose `Content-Type` is not `application/json` with HTTP 415. |
| `body_limit: <bytes>` | `2097152` (2 MiB) | Maximum request body size. |
| `feature_gated: <bool>` | `false` | Gate the server and client on the API crate's own features; see [Dependencies and features](#dependencies-and-features). |

The `Content-Type` check runs before the body is parsed.
A cross-origin `text/plain` POST skips the CORS preflight,
so requiring `application/json` closes that route for cross-site requests.
Set it to `false` only for clients that cannot send the header.

> **Note:** when enabled, the explorer and `openrpc.json` are public by default,
> and they show your method names and schemas.
> Set `docs_require_auth: true` to gate them, or leave the explorer off in production.
> The RPC endpoint itself is never affected by this option.
>
> A browser cannot send an `Authorization` header on a page navigation,
> so a gated explorer opens in a browser only under cookie auth (`.auth_cookie(...)`).
> With bearer tokens only, it is reachable by a client that sets the header.

## Show calls with examples

A schema says what a request may contain; an example shows a call that works.
Give a method an `examples` list in its options block,
and each example appears in the OpenRPC document and in the API explorer.

```rust,ignore
/// Create a user account.
UNAUTHORIZED create_user(CreateUserRequest) -> User {
    examples: [
        /// A user with only the required fields.
        basic {
            params: CreateUserRequest { name: "Ada".into(), email: "ada@example.com".into() },
            result: User { id: 1, name: "Ada".into() },
        },
        /// The e-mail address is already registered.
        duplicate_email {
            params: CreateUserRequest { name: "Ada".into(), email: "taken@example.com".into() },
            error: { code: 1001, message: "Email already registered", data: json!({ "field": "email" }) },
        },
    ],
},
```

Notice that the values are ordinary Rust expressions of the request and response types.
An example that does not fit its type fails to compile,
so the documentation cannot drift away from the code.

Each example has a name, unique within the method, and an optional doc comment
that becomes its description.

* `params` is a value of the request type. Leave it out when the request type is `()`.
* An example has either a `result` or an `error`, never both.
* `error` takes an integer `code`, a string `message` and an optional `data` value of any `Serialize` type.
  The code must be one a handler may send (see below); the macro checks it at compile time.

Values may call functions, such as `params: examples::basic_request()`,
which keeps large values out of the macro.
Keep example expressions deterministic and free of side effects;
the document can be generated more than once.

Examples generate code only when `openrpc` is on.
That code needs `Serialize` on every example type and `DeserializeOwned` on the request type,
in every build that compiles the document functions;
for JSON-RPC that includes client-only and wasm builds.
A request example must also read back unchanged:
it is serialized, deserialized as the request type and serialized again,
and the two JSON values must be equal.
That catches serde renames, defaults and skipped fields that would make the example lie.

Legacy versions in a `versions: [...]` list take no examples.

## Implement the generated trait

The macro generates `UserServiceTrait`, with one method per service method.
Each returns `Result<Response, Box<dyn std::error::Error + Send + Sync>>`.
`WITH_PERMISSIONS` methods receive `&AuthenticatedUser` before the request,
and `OPTIONAL_AUTH` methods receive a `Caller`:

```rust,ignore
use ras_jsonrpc_core::{AuthenticatedUser, Caller};

struct UserServiceImpl;

impl UserServiceTrait for UserServiceImpl {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!("verify credentials and issue a token")
    }

    async fn feed(
        &self,
        caller: Caller,
        _request: (),
    ) -> Result<Feed, Box<dyn std::error::Error + Send + Sync>> {
        let items = match caller.authenticated() {
            Some(user) => vec![format!("Welcome back, {}", user.user_id)],
            None => vec!["Sign in to see your feed".to_string()],
        };
        Ok(Feed { items })
    }

    async fn get_profile(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserProfile {
            user_id: user.user_id.clone(),
        })
    }

    async fn disable_user(
        &self,
        user: &AuthenticatedUser,
        user_id: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        todo!("disable {user_id} on behalf of {}", user.user_id)
    }
}
```

The trait declares each method as returning `impl Future<Output = ...> + Send`,
so writing them as `async fn` works without `async_trait`.

### Errors the client sees

By default a handler error reaches the client as `-32603 "Internal error"` with no detail,
so a database error at the end of a chain of `?`s never leaks internals.
To send an error the client can act on, return a `ras_jsonrpc_types::PublicRpcError` directly:

```rust,ignore
use ras_jsonrpc_types::PublicRpcError;

if email_taken {
    return Err(PublicRpcError::new(1001, "Email already registered")
        .with_data(serde_json::json!({ "field": "email" }))
        .into());
}
```

Only the error the handler returns counts.
A `PublicRpcError` inside another error type, a `thiserror` enum or an `anyhow::Error` is sent as `-32603`,
so an upstream error forwarded with `?` stays hidden;
convert it to a `PublicRpcError` to forward it on purpose.

Use any code outside `-32768..=-32000`, the range JSON-RPC reserves,
or `-32602` for your own validation failures.
Any other reserved code is logged and replaced by `-32603`,
because several of them carry meaning here: the HTTP layer answers `-32001` to `-32004` with 401 or 403.
A public error is sent with HTTP 200, like other JSON-RPC errors.
The generated Rust client returns it as `TransportError::JsonRpc { code, message, data }`.

### Error codes

The generated server answers with these codes:

| Code | Message | Cause | HTTP status |
| --- | --- | --- | --- |
| `-32700` | `Parse error` | The body is not a JSON-RPC request. | 200 |
| `-32600` | `Invalid Request` | `jsonrpc` is not `"2.0"`, or the `Content-Type` check failed. | 200, or 415 for `Content-Type` |
| `-32601` | `Method not found: <name>` | No method has that wire name. | 200 |
| `-32602` | `Invalid params` | The params do not decode as the request type, or a legacy request failed to migrate. | 200 |
| `-32603` | `Internal error` | A handler error that is not a `PublicRpcError`, or a legacy response failed to migrate. | 200 |
| `-32001` | `Authentication required` | No credential for a `WITH_PERMISSIONS` method, or an invalid one. | 401 |
| `-32002` | `Insufficient permissions` | No permission group matched; `data.required` lists the permissions. | 403 |
| `-32003` | `Token expired` | The auth provider reported an expired token. | 401 |
| `-32004` | `CSRF validation failed` | A cookie credential without a valid CSRF token. | 403 |

A credential that is present but invalid is rejected only for `WITH_PERMISSIONS` methods.
`UNAUTHORIZED` and `OPTIONAL_AUTH` methods ignore it, as public REST and file routes do.
Malformed bodies and auth rejections are logged server-side through `tracing`.

### Notifications

A request without an `id` member is a notification.
The server runs the handler and sends no JSON-RPC response:
the HTTP answer is `204 No Content`, or the 401 or 403 status of an auth rejection.
`"id": null` is an ordinary request, answered with a null `id`.
Batch requests, a JSON array of calls, are not supported.

## Build the router

```rust,ignore
let rpc = UserServiceBuilder::new(UserServiceImpl)
    .base_url("/rpc")
    .auth_provider(my_auth_provider)
    .build()?;

let app = axum::Router::new().merge(rpc);
```

`build()` returns `Result<axum::Router, String>`.
It fails when the service has a `WITH_PERMISSIONS` method, or a gated explorer,
and no `.auth_provider(...)` was set;
otherwise every such call would be rejected at runtime.
It also fails when the auth transport configuration is invalid.

The builder methods are:

| Method | Purpose |
| --- | --- |
| `new(service)` | Wraps your trait implementation. |
| `base_url(path)` | The route of the JSON-RPC endpoint. Defaults to `/rpc`. |
| `auth_provider(provider)` | The `AuthProvider` that turns a credential into an `AuthenticatedUser`. |
| `auth_cookie(config)` | Accept a session cookie as well as a bearer token, with double-submit CSRF protection. |
| `csrf_protection(config)` | Replace the CSRF configuration. |
| `auth_transport(config)` | Replace the whole credential transport configuration. |
| `with_usage_tracker(f)` | Async hook called once a call to a declared method passes authorization, before its params are decoded, with the headers (credentials redacted), the user and the request. |
| `with_method_duration_tracker(f)` | Async hook called once the response is built, with the wire method name, the user, the duration and whether the call succeeded. |

A call rejected by authentication or authorization, or to a method the service does not declare,
reaches neither hook.
Every other call reaches both, once each.
`success` is `false` when the response is a JSON-RPC error,
such as invalid params or a handler error.
[Observability](../observability.md) shows how to feed both hooks into OpenTelemetry.

Bearer credentials are read from `Authorization: Bearer <token>`.
[Auth in the API contract](../auth-in-api-contract.md) shows how to write an `AuthProvider`.

## Call it over HTTP

Any HTTP client can call the endpoint with a JSON-RPC 2.0 body:

```bash
curl -X POST http://localhost:3000/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc": "2.0", "method": "sign_in", "params": {"email": "alice@example.com", "password": "secret"}, "id": 1}'
```

A method whose request type is `()` takes no params;
leave `params` out or send `null`.

```bash
curl -X POST http://localhost:3000/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"jsonrpc": "2.0", "method": "get_profile", "id": 2}'
```

## Use the generated Rust client

Enable the shared API crate's `client` feature in the crate that makes outbound calls:

```toml
[dependencies]
my-api = { path = "../api", default-features = false, features = ["client"] }
```

The client calls methods by their Rust names and sends the right wire method.

```rust,ignore
let mut client = UserServiceClientBuilder::new("http://localhost:3000/rpc")
    .with_timeout(std::time::Duration::from_secs(10))
    .build()?;

let signed_in = client
    .sign_in(SignInRequest {
        email: "alice@example.com".to_string(),
        password: "correct horse battery staple".to_string(),
    })
    .await?;

client.set_bearer_token(Some(signed_in.token));

let profile = client.get_profile(()).await?;

client
    .disable_user("user-123".to_string())
    .await?;
```

Each method also has a `<method>_with_timeout(params, timeout)` variant,
and every call returns `Result<Response, ras_transport_core::TransportError>`.
`build()` uses the reqwest transport;
`build_with_transport(transport)` takes any `Arc<dyn HttpTransport>`, such as an in-process test transport.

The same client compiles to WASM when the API crate is built for `wasm32` with `features = ["client"]`.
In a browser, use a relative URL and set the bearer token on a clone before protected calls:

```rust,ignore
let client = UserServiceClientBuilder::new("/rpc").build()?;

let mut authed = client.clone();
authed.set_bearer_token(Some(token));

let profile = authed.get_profile(()).await?;
```

## Versioned methods

Versioning lets a method change its request or response shape
while clients that send the old shape keep working.
It is opt-in; without it, the Rust method name is the wire method.

Give the method a current `version` and `wire` name,
and list each legacy version with its own wire name, types and migration type:

```rust,ignore
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserV1 {
    name: String,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserV2 {
    display_name: String,
    notify: bool,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserResponseV1 {
    name: String,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct RenameUserResponseV2 {
    display_name: String,
    notified: bool,
}

struct RenameUserCompat;

impl ras_jsonrpc_core::VersionMigration<RenameUserV1, RenameUserV2> for RenameUserCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserV1) -> Result<RenameUserV2, Self::Error> {
        Ok(RenameUserV2 {
            display_name: value.name,
            notify: false,
        })
    }
}

impl ras_jsonrpc_core::VersionMigration<RenameUserResponseV2, RenameUserResponseV1>
    for RenameUserCompat
{
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserResponseV2) -> Result<RenameUserResponseV1, Self::Error> {
        Ok(RenameUserResponseV1 {
            name: value.display_name,
        })
    }
}

jsonrpc_service!({
    service_name: UserService,
    openrpc: true,
    methods: [
        UNAUTHORIZED rename_user(RenameUserV2) -> RenameUserResponseV2 {
            version: v2,
            wire: "rename_user.v2",
            versions: [
                v1 {
                    wire: "rename_user.v1",
                    request: RenameUserV1,
                    response: RenameUserResponseV1,
                    migration: RenameUserCompat,
                },
            ],
        },
    ]
});
```

Each legacy entry needs all four fields.
The migration type implements `VersionMigration` twice:
from the legacy request to the current one, and from the current response to the legacy one.
Your handler only ever sees the current types.

The server accepts both `rename_user.v2` and `rename_user.v1`.
A request that fails to migrate is answered with `-32602`, and a response that fails with `-32603`.
The client has `rename_user(...)` for the current method and `rename_user_v1(...)` for the legacy one.
A string label is turned into a method suffix, so `"1.0.0"` gives `rename_user_v1_0_0(...)`.

## OpenRPC and clients

With `openrpc: true`, the macro generates:

```rust,ignore
pub fn generate_userservice_openrpc() -> serde_json::Value;
pub fn try_generate_userservice_openrpc()
    -> Result<serde_json::Value, ras_jsonrpc_types::SpecGenerationError>;
pub fn generate_userservice_openrpc_to_file() -> std::io::Result<()>;
```

The function names use the service name in lower case.
Request and response types must implement `schemars::JsonSchema` (schemars 1.2 or later).
The generated document includes schemas, method names, auth metadata,
permission metadata, version metadata, examples and each method's errors.

`try_generate_*` fails when an example cannot be serialized or does not read back unchanged,
with a `SpecGenerationError` that names the method, the example and the part, never the value.
`generate_*` panics with that error, and `_to_file` returns it as an `io::Error`.
`_to_file` writes to the `openrpc` output path, relative to the working directory.
With `explorer: true`, the served `openrpc.json` is generated once per service and kept;
a failure answers 500 and is logged.

The document describes the call the server accepts.
A method whose request is an object with a fixed set of fields lists one parameter per field,
with `"paramStructure": "by-name"`, so `params` is the request object itself.
A request named descriptors cannot describe, such as an array, a map, an enum,
or a struct with a flattened map or enum, keeps a single parameter named `params`
and is marked `x-ras-params-raw: true`; send that value as `params` directly.
Every method with parameters carries its whole request schema in `x-ras-params-schema`.
A struct whose fields are all optional may be called with `params` left out.

Success examples become standard example pairings.
OpenRPC pairings cannot carry an error, so error examples go into `x-ras-error-examples`.
Each method's `errors` list references the built-in errors it can produce,
then lists the application codes its error examples use.

Auth and versions are described with extensions on each method:

* `x-authentication` is `{ "required": true, "type": "bearer" }` on `WITH_PERMISSIONS` methods,
  and has `"required": false` on `OPTIONAL_AUTH` methods.
* `x-permissions` is a flat list of the permissions a method names,
  and `x-permission-groups` keeps the OR-of-AND groups.
* `x-ras-version` is a method's version label.
  Legacy methods appear as methods of their own, with `x-ras-canonical-version` and `x-ras-canonical-method`
  pointing at the current one.

The client feature emits typed Rust methods for the current methods and for every legacy version.
Each sends its own wire method name,
so old and new clients can coexist while the server migrates requests at the API boundary.

## Generated items

For a service named `UserService`:

| Item | When |
| --- | --- |
| `UserServiceTrait`, `UserServiceBuilder` | server code |
| `userservice_explorer_routes(base_path)` | server code with `explorer` and `openrpc`; `build()` already merges it |
| `UserServiceClient`, `UserServiceClientBuilder` | client code |
| `generate_userservice_openrpc`, `try_generate_userservice_openrpc`, `generate_userservice_openrpc_to_file` | `openrpc` on, in every build |
| `generate_userservice_permission_manifest()`, module `userservice_permissions` | the macro crate's `permissions` feature |

See the runnable service in
[examples/basic-jsonrpc](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/basic-jsonrpc)
and the WASM client usage in
[examples/wasm-ui-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/wasm-ui-demo).
