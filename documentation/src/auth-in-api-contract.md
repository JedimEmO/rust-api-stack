# Auth In The API Contract

RAS puts auth requirements next to the endpoint or method declaration:

```rust,ignore
UNAUTHORIZED health(()) -> HealthStatus,
WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
WITH_PERMISSIONS(["admin"] | ["owner", "editor"]) update_project(UpdateProject) -> Project,
```

This is deliberate. When auth is part of the API definition, generated code can
enforce it consistently and generated API documents can expose it to clients.

## Shared Runtime Model

All service macros integrate with `ras-auth-core`:

```rust,ignore
use ras_auth_core::{AuthFuture, AuthProvider, AuthenticatedUser};

struct MyAuthProvider;

impl AuthProvider for MyAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            // Validate a JWT, API key, session token, or other credential.
            todo!("return an AuthenticatedUser or AuthError")
        })
    }
}
```

`AuthProvider::authenticate` turns a credential into an
`AuthenticatedUser`. The default `check_permissions` implementation requires
that the user has every permission listed in the group, and providers may
override that method for custom policy.

## Auth Syntax

`UNAUTHORIZED` means the generated server does not require a credential for the
operation.

`WITH_PERMISSIONS(["a", "b"])` means the generated server requires a valid
credential and a permission group containing both `a` and `b`.

Groups use OR logic between groups and AND logic within a group:

```rust,ignore
WITH_PERMISSIONS(["admin"] | ["moderator", "editor"])
```

That allows either `admin`, or both `moderator` and `editor`.

An empty group is the authenticated-only form:

```rust,ignore
WITH_PERMISSIONS([])
```

It requires a valid user but no specific permission.

The same syntax is accepted by JSON-RPC, REST, file, and bidirectional JSON-RPC
service macros.

## What Gets Documented

When OpenRPC or OpenAPI generation is enabled, protected operations include
authentication metadata. REST and file services expose bearer auth security
requirements in OpenAPI, and JSON-RPC methods expose `x-authentication`.
Permission names are also emitted as extension metadata so explorer UIs and
client-generation workflows can show what a call requires. `x-permissions`
contains a flattened compatibility list, while `x-permission-groups` preserves
the real OR/AND grouping.
