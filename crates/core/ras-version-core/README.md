# ras-version-core

The `VersionMigration` trait, which converts between an old and a current version of an API type.

When an endpoint changes its request or response shape,
older clients still send and expect the old one.
`rest_service!` and `jsonrpc_service!` can keep serving those clients:
a versioned endpoint lists its legacy versions,
and each legacy version names a migration type.
The generated server upgrades a legacy request to the current type,
calls the one handler you wrote, and downgrades the response again.
The migration type implements `VersionMigration` once in each direction.

## Example

```rust
use ras_version_core::VersionMigration;

struct CreateUserV1 {
    name: String,
}

struct CreateUserV2 {
    display_name: String,
    send_welcome_email: bool,
}

struct CreateUserMigration;

impl VersionMigration<CreateUserV1, CreateUserV2> for CreateUserMigration {
    type Error = std::convert::Infallible;

    fn migrate(value: CreateUserV1) -> Result<CreateUserV2, Self::Error> {
        Ok(CreateUserV2 {
            display_name: value.name,
            send_welcome_email: true,
        })
    }
}
```

`migrate` is an associated function with no `self`,
so a migration is a plain conversion and carries no state.
`Error` must implement `Display`.
When a request migration fails, the generated server rejects the call:
REST answers `400` with the error's `Display` text as the message,
and JSON-RPC answers with a generic invalid-params error.
A failed response migration is a server error in both.

You rarely depend on this crate directly.
`ras-rest-core` and `ras-jsonrpc-core` re-export the trait,
and the generated code refers to it through them.

```toml
[dependencies]
ras-version-core = "0.1"
```

The [REST versioning](../../../documentation/src/macros/rest-service.md#versioning)
and [JSON-RPC versioned methods](../../../documentation/src/macros/jsonrpc-service.md#versioned-methods)
sections show the macro syntax.

## Checks

```bash
cargo test -p ras-version-core --locked
cargo clippy -p ras-version-core --all-targets --all-features --locked -- -D warnings
```
