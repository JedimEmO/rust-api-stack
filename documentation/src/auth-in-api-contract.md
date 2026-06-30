# Auth In The API Contract

RAS puts auth requirements next to the endpoint or method declaration:

```rust,ignore
UNAUTHORIZED health(()) -> HealthStatus,
OPTIONAL_AUTH feed(()) -> Feed,
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

`OPTIONAL_AUTH` means the operation is **public, but opportunistically
identified**. The route is never rejected for auth reasons; instead the handler
receives a `ras_auth_core::Caller` as its first argument:

```rust,ignore
pub enum Caller {
    Anonymous,
    Authenticated(AuthenticatedUser),
}
```

Resolution is **fully lenient**: a missing credential, an invalid or expired
token, a missing auth provider, or a cookie credential that fails CSRF on an
unsafe method (`POST`/`PUT`/`PATCH`/`DELETE`) all resolve to `Caller::Anonymous`
— a forged or stale credential simply executes as the public path. A valid
credential resolves to `Caller::Authenticated(user)`. No permission check is
performed. Reach for it on "public, but richer when signed in" endpoints
(per-document ACLs, personalization, author previews). The `file_service!`
macro surfaces the same optional caller through `FileRequestContext` rather than
a `Caller` parameter.

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

`OPTIONAL_AUTH` operations advertise an **optional** security requirement: REST
and file services emit `security: [{}, { "bearerAuth": [] }]` (anonymous is
acceptable, and a bearer token is honoured), and JSON-RPC methods emit
`x-authentication` with `"required": false`.
