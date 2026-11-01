# `jsonrpc_service!`

Use `jsonrpc_service!` when you want an HTTP JSON-RPC API with typed request
and response payloads, generated server dispatch, generated Rust clients, and
optional OpenRPC output.

## Dependencies And Features

Put the macro in the shared API definition crate. If you want server and client
outputs to stay optional, expose API-crate features that forward to the macro
crate features and enable the runtime dependencies those generated surfaces
refer to.

```toml
[dependencies]
ras-jsonrpc-macro = { version = "0.4.0", default-features = false }
ras-jsonrpc-types = "0.3.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"
ras-transport-core = { version = "0.2.0", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-jsonrpc-core = { version = "0.3.0", optional = true }
axum = { version = "0.8", optional = true }
tokio = { version = "1.0", features = ["full"], optional = true }

[features]
default = []
server = ["ras-jsonrpc-macro/server", "dep:ras-jsonrpc-core", "dep:axum", "dep:tokio"]
client = ["ras-jsonrpc-macro/reqwest", "ras-transport-core/reqwest"]
```

Server binaries then depend on `my-api` with `features = ["server"]`; clients
depend on the same API crate with `features = ["client"]`. The generated code
itself is selected by the `ras-jsonrpc-macro` features, not by generated
consumer-crate cfg attributes.

The macro crate's `client` feature emits the generated client types and
`build_with_transport(...)`. Its `reqwest` feature also emits the default
reqwest-backed `build()`. If a crate only injects a custom transport, forward
`ras-jsonrpc-macro/client` plus `dep:ras-transport-core` instead of
`ras-jsonrpc-macro/reqwest`.

## Define The Service

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
pub struct UserProfile {
    pub user_id: String,
}

jsonrpc_service!({
    service_name: UserService,
    openrpc: true,
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        OPTIONAL_AUTH feed(FeedRequest) -> FeedResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS(["admin"] | ["support", "users:write"]) disable_user(String) -> (),
    ]
});
```

The Rust method name is the JSON-RPC wire method unless a versioned method block
sets an explicit `wire` name. The auth requirement is one of `UNAUTHORIZED`,
`OPTIONAL_AUTH`, or `WITH_PERMISSIONS([...])`; see
[Auth In The API Contract](../auth-in-api-contract.md).

## Service Options

Optional service-level fields (alongside `service_name` / `methods`):

| Option | Default | Meaning |
| --- | --- | --- |
| `require_json_content_type: <bool>` | `true` | Reject a non-`application/json` request with `415` before parsing. |
| `body_limit: <bytes>` | `2 * 1024 * 1024` | Maximum request body size. |
| `openrpc: true` / `explorer: true` | off | Emit the OpenRPC document and host the API explorer. |
| `docs_require_auth: <bool>` | `false` | Gate the explorer page and `openrpc.json` behind authentication (any authenticated user). |
| `feature_gated: <bool>` | `false` | Wrap the server/client in the consumer crate's own `server`/`client` features. |

The JSON-RPC endpoint always uses `application/json`; the default
`require_json_content_type` check rejects a cross-origin `text/plain` POST (a
CORS-safelisted content type that skips preflight) before the body is parsed. Set
it to `false` only for clients that cannot send the header.

> **Note:** the explorer and `openrpc.json` are served **without** authentication
> by default when enabled, exposing your method names and schemas. Set
> `docs_require_auth: true` to gate them (the RPC endpoint itself is never gated
> by this option), or leave the explorer disabled in production.
>
> Because a browser top-level navigation cannot send an `Authorization` header,
> the gated explorer is only reachable in a browser under **cookie** auth
> (`.auth_cookie(...)`); on a bearer-only transport it is reachable only by a
> programmatic client that sets the header.

A malformed JSON body and authentication/authorization rejections are logged
server-side via `tracing`. A service that declares any `WITH_PERMISSIONS` method
(or a gated explorer) but is built without `.auth_provider(...)` fails `build()`
with a clear error instead of rejecting every such call at runtime.

## Show Calls With Examples

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
An example that no longer fits its type fails to compile,
so the documentation cannot drift away from the code.

Each example has a name, unique within the method, and an optional doc comment
that becomes its description.

* `params` is a value of the request type. Leave it out when the request type is `()`.
* An example has either a `result` or an `error`, never both.
* `error` takes an integer `code`, a string `message` and an optional `data` value.
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

## Implement The Generated Trait

Protected (`WITH_PERMISSIONS`) methods receive `&AuthenticatedUser` before their
request payload; `OPTIONAL_AUTH` methods receive a `ras_jsonrpc_core::Caller`
instead (the request is never rejected for auth reasons):

```rust,ignore
struct UserServiceImpl;

impl UserServiceTrait for UserServiceImpl {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!("verify credentials and issue token")
    }

    async fn get_profile(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserProfile {
            user_id: user.user_id.clone(),
        })
    }
}
```

### Errors The Client Sees

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

## Build The Router

```rust,ignore
let rpc = UserServiceBuilder::new(UserServiceImpl)
    .base_url("/rpc")
    .auth_provider(my_auth_provider)
    .build()?;

let app = axum::Router::new().merge(rpc);
```

The generated server extracts bearer credentials from `Authorization`, can be
configured for secure session cookies, routes by JSON-RPC method name, parses
typed params, checks auth, and converts handler errors into JSON-RPC error
responses.

## Use The Generated Rust Client

Enable the shared API crate's `client` feature in the crate that makes outbound
calls:

```toml
[dependencies]
my-api = { path = "../api", default-features = false, features = ["client"] }
```

The generated client calls methods by their Rust names and sends the correct
JSON-RPC wire method internally.

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

For browser/WASM clients, use the same generated client with a browser URL and
set the bearer token on a cloned client before protected calls:

```rust,ignore
let client = UserServiceClientBuilder::new("/rpc").build()?;

let mut authed = client.clone();
authed.set_bearer_token(Some(token));

let profile = authed.get_profile(()).await?;
```

## OpenRPC And Clients

With `openrpc: true`, the macro generates:

```rust,ignore
pub fn generate_userservice_openrpc() -> serde_json::Value;
pub fn try_generate_userservice_openrpc()
    -> Result<serde_json::Value, ras_jsonrpc_types::SpecGenerationError>;
pub fn generate_userservice_openrpc_to_file() -> Result<(), std::io::Error>;
```

Request and response types must implement `schemars::JsonSchema` (schemars 1.2 or later).
The generated document includes schemas, method names, auth metadata,
permission metadata, version metadata, examples and each method's errors.

`try_generate_*` fails when an example cannot be serialized or does not read back unchanged,
with a `SpecGenerationError` that names the method, the example and the part, never the value.
`generate_*` panics with that error, and `_to_file` returns it as an `io::Error`.
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

The API crate's `client` feature emits typed Rust methods for the current
operation names and, when a method declares versioned compatibility, for the
legacy Rust method aliases too. Each generated method still sends the configured
wire method name, so old and new clients can coexist while the server migrates
requests at the API boundary.

Browser clients can compile to WASM when the API crate dependency is enabled
with `features = ["client"]` for `wasm32`.

See the runnable service in
[examples/basic-jsonrpc](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/basic-jsonrpc)
and the WASM client usage in
[examples/wasm-ui-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/wasm-ui-demo).
