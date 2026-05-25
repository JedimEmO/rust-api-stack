# ras-permission-manifest

Typed permission manifest data structures for Rust Agent Stack service definitions.

Service macros emit `ServicePermissions` values from their API definitions. Build scripts can combine those values into a deterministic `PermissionManifest` JSON artifact, while token issuing code can use generated permission constants with `PermissionSet`.
