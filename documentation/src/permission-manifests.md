# Permission manifests

A service definition says, for every operation,
whether it is public, needs a login, or needs certain permissions.
The generated server enforces that.
Other code needs the same facts:
an audit wants a list, an admin UI wants to show it,
and the code that issues tokens has to spell the same permission strings.
Copying those strings by hand lets them drift from the service.

With the `permissions` feature, the service macros also emit the auth declarations as data:
a manifest function, and constants for every permission and every protected operation.
The types live in `ras-permission-manifest`.

## Enabling the feature

Turn on `permissions` in the macro crate.
The generated code refers to `ras-permission-manifest` by name,
so add that crate next to the macro:

```toml
[dependencies]
ras-rest-macro = { version = "0.4.0", default-features = false, features = ["permissions"] }
ras-permission-manifest = "0.1.0"
```

The other service macros have the same feature:

```toml
ras-jsonrpc-macro = { version = "0.4.0", default-features = false, features = ["permissions"] }
ras-file-macro = { version = "0.3.0", default-features = false, features = ["permissions"] }
ras-jsonrpc-bidirectional-macro = { version = "0.3.0", default-features = false, features = ["permissions"] }
```

The feature belongs to the macro crate.
The macro emits the manifest function and the constants only when it is on,
and the generated code does not check a `permissions` feature in your API crate.
The output does not depend on the macro's `server` or `client` features either,
so a client build of the API crate sees the same constants.

## Generated items

For a REST service named `UserService`, the macro emits:

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

The function and module names use the service name in lower case.

Each permission string used anywhere in the service gets a `PermissionRef` constant.
The constant name is the string in upper case,
with every run of other characters turned into `_`:
`task:write` becomes `TASK_WRITE`.
When two strings map to the same name, both names get a hash suffix.

Each protected operation gets a `StaticPermissionRequirement` constant in `operations`,
with the same OR-of-AND groups the server checks.
Public and `OPTIONAL_AUTH` operations get none.

A caller meets a requirement by holding every permission of one group.
With no groups, or only empty ones, any authenticated caller does.
An empty group next to non-empty ones is ignored, as it is on the server,
so `["admin"] | []` requires `admin`.
The service macros reject that shape anyway;
the rule matters when code builds requirements by hand.
The constant is named after the operation:

| Macro | Constant name from |
|---|---|
| `rest_service!` | the handler name, such as `post_users` for `POST users` |
| `jsonrpc_service!` | the method name |
| `file_service!` | the endpoint name |
| `jsonrpc_bidirectional_service!` | `client_to_server_<method>`; server-to-client calls take no permissions, so they have no constant |

A versioned operation also gets one constant per old version, with the version appended.

## Writing the manifest from a build script

Combine the services into one `PermissionManifest` and write it as JSON:

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

The build script needs the API crate and `ras-permission-manifest` as build dependencies:

```toml
[build-dependencies]
workspace-api = { path = "../workspace-api" }
ras-permission-manifest = "0.1.0"
```

The manifest functions need no API crate feature.
If the same build script also writes the OpenAPI or OpenRPC document,
enable the API crate feature that turns on the macro's `server` feature, usually `server`.

A build script runs in the package directory,
so the relative path above lands in that package's `target/` directory,
not necessarily the workspace one.

`from_services` sorts services and operations, so the file only changes when the auth declarations do.
An entry looks like this:

```json
{
  "operation_id": "UserService.post_users",
  "operation_name": "post_users",
  "kind": "rest_endpoint",
  "wire": { "kind": "rest", "method": "POST", "path": "/api/v1/users" },
  "auth": { "type": "permissions", "any_of": [{ "all_of": ["admin"] }] },
  "version": null,
  "canonical_operation_id": null
}
```

`auth` is one of `public`, `optional` (`OPTIONAL_AUTH`), `authenticated`, or `permissions`.
A versioned operation appears once per callable wire method or path;
its old versions carry the `version` and point at the current operation through `canonical_operation_id`.
The top-level `schema_version` field lets tooling check that it understands the format.

## Issuing tokens with the constants

The permission constants are useful where tokens are issued.
`ras-identity-session` asks a `UserPermissions` implementation
for the permissions of each identity that signs in.
Build that list from the generated constants:

```rust,ignore
use async_trait::async_trait;
use ras_identity_core::{IdentityResult, UserPermissions, VerifiedIdentity};
use ras_identity_session::SessionService;
use ras_permission_manifest::PermissionSet;
use std::sync::Arc;
use workspace_api::userservice_permissions;

struct WorkspacePermissions;

#[async_trait]
impl UserPermissions for WorkspacePermissions {
    async fn get_permissions(&self, identity: &VerifiedIdentity) -> IdentityResult<Vec<String>> {
        let mut permissions = PermissionSet::new().with(userservice_permissions::TASK_WRITE);
        if identity.subject == "admin" {
            permissions.insert(userservice_permissions::ADMIN);
        }
        Ok(permissions.into_hash_set().into_iter().collect())
    }
}

let session_service =
    SessionService::new(session_config)?.with_permissions(Arc::new(WorkspacePermissions));
```

A misspelled constant is a compile error, where a misspelled string would be a token that never passes a check.

The operation constants test a permission set against an operation,
which suits unit tests for a role's grants:

```rust,ignore
let permissions = PermissionSet::new()
    .with(userservice_permissions::ADMIN)
    .into_hash_set();

assert!(userservice_permissions::operations::POST_USERS.is_satisfied_by(&permissions));
```

The manifest does not replace the runtime checks.
Tokens and `AuthenticatedUser` still carry plain strings,
and the generated server checks them with the configured `AuthProvider`.
