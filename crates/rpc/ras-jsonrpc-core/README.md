# ras-jsonrpc-core

Runtime support for the JSON-RPC servers that `jsonrpc_service!` from
[`ras-jsonrpc-macro`](../ras-jsonrpc-macro) generates.

The generated server code needs auth types, the version migration trait and `tracing`,
which live in three different crates.
This crate re-exports them under one path, `ras_jsonrpc_core`,
so your API crate declares one dependency for them instead of three.
It has no code of its own.
The generated code still names `ras-jsonrpc-types` directly for the wire types,
which clients use too, so an API crate depends on both.

Your own server code uses it too:
to implement `AuthProvider`, to name `AuthenticatedUser` and `Caller` in handler signatures,
and to configure cookie auth on the generated builder.

## What it re-exports

| From | Main items |
|---|---|
| [`ras-auth-core`](../../core/ras-auth-core) | `AuthProvider`, `AuthenticatedUser`, `Caller`, `AuthError`, `AuthResult`, `AuthFuture`, `AuthTransportConfig`, `AuthCookieConfig`, `CsrfConfig`, and the credential and CSRF helpers |
| [`ras-jsonrpc-types`](../ras-jsonrpc-types) | `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`, `error_codes`, `PublicRpcError`, `HandlerErrorOutcome`, `SpecGenerationError` |
| [`ras-version-core`](../../core/ras-version-core) | `VersionMigration` |
| `tracing` | the crate itself, as `ras_jsonrpc_core::tracing` |

The crate has no Cargo features.

## Installation

```toml
[dependencies]
ras-jsonrpc-core = "0.3"
```

An API crate that also builds clients, including wasm clients,
usually makes this dependency optional and enables it from its `server` feature.
The [`jsonrpc_service!` guide](../../../documentation/src/macros/jsonrpc-service.md#dependencies-and-features)
shows that setup.

## Implementing an auth provider

The generated server hands the credential to an `AuthProvider`.
`authenticate` turns a token into an `AuthenticatedUser`, or fails with an `AuthError`.

```rust
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::collections::{HashMap, HashSet};

struct StaticTokenAuthProvider {
    users_by_token: HashMap<String, AuthenticatedUser>,
}

impl AuthProvider for StaticTokenAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            self.users_by_token
                .get(&token)
                .cloned()
                .ok_or(AuthError::InvalidToken)
        })
    }
}

let mut users_by_token = HashMap::new();
users_by_token.insert(
    "admin-token".to_string(),
    AuthenticatedUser {
        user_id: "admin-user".to_string(),
        permissions: HashSet::from(["admin".to_string(), "user".to_string()]),
        metadata: None,
    },
);

let auth_provider = StaticTokenAuthProvider { users_by_token };
```

`check_permissions` has a default implementation:
the user passes when they hold every permission in the list.
`WITH_PERMISSIONS(["a"] | ["b"])` checks each group in turn.
Override `check_permissions` when permissions depend on a tenant, a role or an external policy.

For JWT sessions, `JwtAuthProvider` from `ras-identity-session` implements this trait;
see the [identity and sessions chapter](../../../documentation/src/identity-and-sessions.md).

`AuthError` has five variants:
`InvalidToken`, `TokenExpired`, `InsufficientPermissions { required, has }`,
`AuthenticationRequired` and `Internal(String)`.
It is a server-side type and does not implement `Serialize`.
Its `Display` text names the required permissions and only the *number* of permissions the caller holds,
so logging it does not leak the caller's grants.
The generated server answers `TokenExpired` with `-32003`, any other authentication failure with `-32001`,
and a failed permission check with `-32002`, and never sends `has` to the client.

## Using it with the generated service

```rust
use ras_jsonrpc_core::{AuthenticatedUser, Caller};
use ras_jsonrpc_macro::jsonrpc_service;

jsonrpc_service!({
    service_name: MyService,
    methods: [
        UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
        OPTIONAL_AUTH feed(FeedRequest) -> FeedResponse,
        WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
        WITH_PERMISSIONS(["admin"]) delete_user(UserId) -> (),
    ]
});

struct MyServiceImpl;

impl MyServiceTrait for MyServiceImpl {
    async fn sign_in(
        &self,
        request: SignInRequest,
    ) -> Result<SignInResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!("check the credentials and issue a token")
    }

    async fn feed(
        &self,
        caller: Caller,
        request: FeedRequest,
    ) -> Result<FeedResponse, Box<dyn std::error::Error + Send + Sync>> {
        todo!("show more when caller.authenticated() is Some")
    }

    async fn get_profile(
        &self,
        user: &AuthenticatedUser,
        _request: (),
    ) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        todo!("load the profile of user.user_id")
    }

    async fn delete_user(
        &self,
        user: &AuthenticatedUser,
        request: UserId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        todo!("only callers holding `admin` get here")
    }
}

// The JSON-RPC route defaults to `/rpc`.
let router: axum::Router = MyServiceBuilder::new(MyServiceImpl)
    .base_url("/api/rpc")
    .auth_provider(auth_provider)
    .build()?;
```

Notice how the handler signature follows the auth level.
`UNAUTHORIZED` handlers get only the request,
`OPTIONAL_AUTH` handlers get a `Caller` that may be anonymous,
and `WITH_PERMISSIONS` handlers get the `AuthenticatedUser` that passed the check.

A handler error reaches the client as `-32603 "Internal error"` with no detail,
unless the handler returns a `PublicRpcError`;
see [`ras-jsonrpc-types`](../ras-jsonrpc-types/README.md#errors-a-handler-sends-to-the-client).

## Cookie auth

Browser clients can send the token in a secure cookie instead of a bearer header.
The auth provider still receives the token string:

```rust
use ras_jsonrpc_core::AuthCookieConfig;

let router = MyServiceBuilder::new(MyServiceImpl)
    .auth_provider(auth_provider)
    .auth_cookie(AuthCookieConfig::default())
    .build()?;
```

Bearer tokens stay enabled; when a request carries both, the bearer token is used.
`auth_cookie` also installs the default double-submit CSRF check,
because a browser attaches the cookie to cross-site requests too.
A cookie-authenticated call must then send an `x-ras-csrf` header
equal to the value of the `__Host-ras-csrf` cookie.
Pass a different `CsrfConfig` to `csrf_protection` to change that check,
or a whole `AuthTransportConfig` to `auth_transport`.

## Version migrations

A method can keep an old wire format alive next to the current one.
The macro calls a `VersionMigration` to turn the old request into the current one,
runs the current handler, and migrates the response back:

```rust
use ras_jsonrpc_core::VersionMigration;

struct RenameCompat;

impl VersionMigration<RenameUserV1, RenameUserV2> for RenameCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserV1) -> Result<RenameUserV2, Self::Error> {
        Ok(RenameUserV2 {
            display_name: value.name,
            notify: false,
        })
    }
}

impl VersionMigration<RenameUserResponseV2, RenameUserResponseV1> for RenameCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserResponseV2) -> Result<RenameUserResponseV1, Self::Error> {
        Ok(RenameUserResponseV1 {
            name: value.display_name,
        })
    }
}
```

The [`jsonrpc_service!` guide](../../../documentation/src/macros/jsonrpc-service.md#versioned-methods) shows how a method declares its versions.

## Where to go next

- The [`jsonrpc_service!` guide](../../../documentation/src/macros/jsonrpc-service.md) covers the whole macro.
- [`examples/basic-jsonrpc`](../../../examples/basic-jsonrpc) is a runnable service.

## Checks

```bash
cargo test -p ras-jsonrpc-core --locked
cargo clippy -p ras-jsonrpc-core --all-targets --all-features --locked -- -D warnings
```

## License

This project is licensed under either MIT or Apache-2.0.
