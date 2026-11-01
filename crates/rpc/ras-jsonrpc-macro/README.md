# ras-jsonrpc-macro

The `jsonrpc_service!` macro: one definition of a JSON-RPC 2.0 service,
from which it generates the axum server, a typed Rust client and an OpenRPC document.

A hand-written JSON-RPC API keeps its contract in several places at once.
The dispatch table, the auth checks, the client and the API document each repeat the method names and types,
and nothing stops them from drifting apart.

`jsonrpc_service!` keeps the contract in one place.
Each method declares its auth requirement, request type and response type,
and the macro generates the rest from that declaration.

```rust
use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SignInRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SignInResponse {
    pub token: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct UserProfile {
    pub user_id: String,
}

jsonrpc_service!({
    service_name: UserService,
    openrpc: true,
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
    ]
});
```

From this the macro generates:

* `UserServiceTrait`, with one async method per JSON-RPC method for you to implement.
  `get_profile` receives the `&AuthenticatedUser` that passed the permission check.
* `UserServiceBuilder`, which turns your implementation and an `AuthProvider` into an `axum::Router`.
* `UserServiceClient`, with typed `sign_in(...)` and `get_profile(...)` methods.
* `generate_userservice_openrpc()` and its siblings, which build the OpenRPC document.
  `explorer: true` also serves an API explorer next to the endpoint.

Serving the service takes a trait implementation and a builder call:

```rust,ignore
use ras_jsonrpc_core::AuthenticatedUser;

struct Users;

impl UserServiceTrait for Users {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!("check the password and issue a token")
    }

    async fn get_profile(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        Ok(UserProfile { user_id: user.user_id.clone() })
    }
}

let app = UserServiceBuilder::new(Users)
    .base_url("/rpc")
    .auth_provider(my_auth_provider)
    .build()?;
```

A handler error reaches the client as `-32603 "Internal error"`,
unless the handler returns a `ras_jsonrpc_types::PublicRpcError` with a code the client may act on.

## Installation

The generated code refers to runtime crates by name,
so a crate that calls the macro depends on them as well.
For one crate with both the server and the client:

```toml
[dependencies]
ras-jsonrpc-macro = "0.4"
ras-jsonrpc-core = "0.3"
ras-jsonrpc-types = "0.3"
ras-transport-core = { version = "0.2", features = ["reqwest"] }
axum = "0.8"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "1.2"
```

The macro crate's features choose what it generates:

| Feature | Default | Emits |
| --- | --- | --- |
| `server` | yes | The handler trait and the router builder. |
| `client` | through `reqwest` | The client, with `build_with_transport(...)`. |
| `reqwest` | yes | `client`, plus the reqwest-backed `build()`. |
| `permissions` | no | A permission manifest, using `ras-permission-manifest`. |

Most projects put the definition in a shared API crate,
with `server` and `client` features that the backend and the clients pick from.
The guide shows that layout, and the client compiles to WASM.

## Documentation

The [`jsonrpc_service!` guide](https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/macros/jsonrpc-service.md)
is the full reference: method and service options, auth requirements, examples in the OpenRPC document,
error codes, notifications, versioned methods and the generated client.

Runnable code is in
[examples/basic-jsonrpc](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/basic-jsonrpc)
and [examples/wasm-ui-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/wasm-ui-demo).

## License

Licensed under either MIT or Apache-2.0, at your option.
