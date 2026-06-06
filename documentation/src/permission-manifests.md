# Permission Manifests

The service macros can emit a typed permission manifest from the same auth
declarations used by the generated server. This is useful for audits, admin UI
tooling, test assertions, and token issuing code that should not repeat
permission strings by hand.

## Enable The Feature

Enable manifest generation on the macro crate. The generated API refers to
`ras-permission-manifest`, so add that crate beside the macro dependency:

```toml
[dependencies]
ras-rest-macro = { version = "0.2.1", default-features = false, features = ["permissions"] }
ras-permission-manifest = "0.1.0"
```

For file services and JSON-RPC services, use the equivalent macro crate:

```toml
ras-file-macro = { version = "0.1.0", default-features = false, features = ["permissions"] }
ras-jsonrpc-macro = { version = "0.2.0", default-features = false, features = ["permissions"] }
```

The `permissions` switch belongs to the macro crate. The macro emits the
manifest functions and constants only when that macro feature is enabled; the
generated code does not branch on a `permissions` feature in your API crate.

If your API crate exposes optional server/client outputs, those features can
forward to the macro crate:

```toml
[features]
server = ["ras-rest-macro/server", "dep:axum", "dep:ras-rest-core"]
client = ["ras-rest-macro/reqwest", "ras-transport-core/reqwest"]
```

Server build scripts then depend on the API crate feature that makes the
generated service/spec functions available:

```toml
[build-dependencies]
workspace-api = { path = "../workspace-api", features = ["server"] }
ras-permission-manifest = "0.1.0"
```

## Generated API

For a service named `UserService`, the macro emits:

```rust,ignore
pub fn generate_userservice_permission_manifest()
    -> ras_permission_manifest::ServicePermissions;

pub mod userservice_permissions {
    pub const ADMIN: ras_permission_manifest::PermissionRef;
    pub const TASK_WRITE: ras_permission_manifest::PermissionRef;

    pub mod operations {
        pub const POST_USERS: ras_permission_manifest::StaticPermissionRequirement;
    }
}
```

Permission constants are generated from every permission string used by the
service. Operation constants are generated for protected operations and preserve
the same OR/AND grouping used by runtime checks.

## Build-Time Artifact

Aggregate service manifests explicitly from `build.rs`:

```rust,ignore
fn main() {
    let manifest = ras_permission_manifest::PermissionManifest::from_services([
        workspace_api::generate_userservice_permission_manifest(),
        workspace_api::generate_documentservice_permission_manifest(),
    ]);

    ras_permission_manifest::write_manifest(
        "target/ras-permissions/workspace.json",
        &manifest,
    )
    .expect("write permission manifest");
}
```

The JSON distinguishes public operations, authenticated-only operations, and
permission groups. For versioned compatibility endpoints, every callable wire
method or path appears in the manifest.

## Token Issuing

Use generated constants when constructing permission claims:

```rust,ignore
use ras_permission_manifest::PermissionSet;
use workspace_api::userservice_permissions;

let permissions = PermissionSet::new()
    .with(userservice_permissions::TASK_WRITE)
    .with(userservice_permissions::ADMIN)
    .into_hash_set();

session_service.begin_session(user_id, permissions).await?;
```

You can also test whether a candidate token satisfies a generated operation
requirement:

```rust,ignore
assert!(
    userservice_permissions::operations::POST_USERS
        .is_satisfied_by(&permissions)
);
```

The manifest does not replace the runtime auth model. JWT/session claims still
carry strings, but token issuing code can now import compile-checked constants
from the API contract instead of spelling those strings repeatedly.
