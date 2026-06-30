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
ras-jsonrpc-macro = { version = "0.2.0", default-features = false }
ras-jsonrpc-types = "0.1.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.0.0-alpha.20"
ras-transport-core = { version = "0.1.0", optional = true }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
ras-jsonrpc-core = { version = "0.1.2", optional = true }
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
pub fn generate_userservice_openrpc_to_file() -> Result<(), std::io::Error>;
```

Request and response types must implement `schemars::JsonSchema` for OpenRPC
generation. The generated document includes schemas, method names, auth
metadata, permission metadata, and version metadata.

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
