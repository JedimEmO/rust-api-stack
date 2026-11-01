# ras-permission-manifest

Data types that describe which permissions each operation of a RAS service requires.

A service definition already says, for every operation,
whether it is public, needs a login, or needs certain permissions.
Other code needs the same facts:
an audit wants a list, an admin UI wants to show it,
and the code that issues tokens has to spell the same permission strings.
Copying the strings by hand lets them drift from the service.

With the `permissions` feature of `ras-rest-macro`, `ras-jsonrpc-macro`, `ras-file-macro`
or `ras-jsonrpc-bidirectional-macro`, each service definition also generates:

- a `generate_<service>_permission_manifest()` function that returns a `ServicePermissions`,
- a `<service>_permissions` module with one `PermissionRef` constant per permission string,
- and, inside it, an `operations` module with one `StaticPermissionRequirement` constant per protected operation.

The generated code refers to this crate by name, so an API crate that enables the feature adds it as a dependency:

```toml
[dependencies]
ras-rest-macro = { version = "0.4", default-features = false, features = ["permissions"] }
ras-permission-manifest = "0.1"
```

## Main types

| Item | Use |
|---|---|
| `PermissionManifest` | Several services in one document. `from_services` sorts services and operations, so the output is stable. |
| `write_manifest` | Writes a manifest as pretty-printed JSON, creating the parent directory. |
| `ServicePermissions`, `OperationPermissions` | One service, and one callable operation with its wire method or path. |
| `AuthRequirementInfo` | `Public`, `Optional` (`OPTIONAL_AUTH`), `Authenticated`, or `Permissions { any_of }`. |
| `PermissionRef` | A generated constant for one permission string. |
| `StaticPermissionRequirement` | A generated constant for one operation; `is_satisfied_by` tests a permission set against it. |
| `PermissionSet` | Collects `PermissionRef` constants into the `HashSet<String>` a token or session carries. |

A requirement is a list of groups.
A caller passes when they hold every permission in at least one group.

`SCHEMA_VERSION` is written into every manifest.
Tooling can compare it before reading the rest of the document.

The crate depends only on `serde` and `serde_json`, and has no Cargo features.

## Where to go next

The [permission manifests chapter](../../../documentation/src/permission-manifests.md)
shows the generated items, a build script that writes the manifest, and token issuing with the constants.
