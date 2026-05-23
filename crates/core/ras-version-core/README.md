# ras-version-core

Small core crate for explicit API version migrations.

Rust Agent Stack service macros use `VersionMigration` when a legacy request
or response type needs to be converted to or from the canonical type for an
endpoint. Keeping the conversion in a trait makes version compatibility paths
visible, testable, and independent from transport-specific code.

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

## Checks

```bash
cargo test -p ras-version-core --locked
cargo clippy -p ras-version-core --all-targets --all-features --locked -- -D warnings
```
