# Auth in the API contract

RAS puts the auth requirement next to each endpoint or method declaration:

```rust,ignore
UNAUTHORIZED health(()) -> HealthStatus,
OPTIONAL_AUTH feed(()) -> Feed,
WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
WITH_PERMISSIONS(["admin"] | ["owner", "editor"]) update_project(UpdateProject) -> Project,
```

This is deliberate.
When auth is part of the API definition, the generated code enforces it the same way on every route,
and the generated API documents show it to clients.

## The shared runtime model

All service macros use the `AuthProvider` trait from `ras-auth-core`:

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

`AuthProvider::authenticate` turns a credential into an `AuthenticatedUser`.
The default `check_permissions` requires the user to hold every permission in a group.
A provider may override it for its own policy.

## The auth keywords

`UNAUTHORIZED` means the generated server requires no credential for the operation.
It does not look at one either, so a stale or invalid credential does not turn a public call into an auth error.

`OPTIONAL_AUTH` means the operation is public, but identifies the caller when it can.
The route is never rejected for auth reasons.
Instead the handler receives a `ras_auth_core::Caller` as its first argument:

```rust,ignore
pub enum Caller {
    Anonymous,
    Authenticated(AuthenticatedUser),
}
```

A valid credential resolves to `Caller::Authenticated(user)`.
Everything else resolves to `Caller::Anonymous`:
a missing credential, an invalid or expired token, a missing auth provider,
or a cookie credential that fails the CSRF check on an unsafe method (`POST`, `PUT`, `PATCH` or `DELETE`).
So a forged or stale credential runs the public path, never an error path.
No permission check runs.

Use it for endpoints that are public but richer for a signed-in user,
such as per-document access rules, personalization or author previews.
`file_service!` hands the optional caller to the handler through `FileRequestContext::user`
instead of a `Caller` argument.

`WITH_PERMISSIONS(["a", "b"])` means the generated server requires a valid credential,
and the user must hold both `a` and `b`.

Groups combine with OR between groups and AND within a group:

```rust,ignore
WITH_PERMISSIONS(["admin"] | ["moderator", "editor"])
```

That allows either `admin`, or both `moderator` and `editor`.

An empty group means "any signed-in user":

```rust,ignore
WITH_PERMISSIONS([])
```

It requires a valid credential but no specific permission.

The JSON-RPC, REST, file and bidirectional JSON-RPC macros accept the same syntax.
The one exception is a bidirectional `server_to_client_calls` entry.
The server makes that call, so there is no caller to identify or check,
and `OPTIONAL_AUTH` and `WITH_PERMISSIONS` there are compile errors.

## What gets documented

When OpenRPC or OpenAPI generation is on, protected operations carry their auth metadata.
REST and file services list a bearer-auth security requirement in OpenAPI,
and JSON-RPC methods carry an `x-authentication` extension.
Permission names go into extension fields,
so explorer UIs and client generators can show what a call requires.
`x-permissions` is a flat list of every permission named,
and `x-permission-groups` keeps the real OR and AND grouping.

`OPTIONAL_AUTH` operations advertise an optional security requirement.
REST and file services emit `security: [{}, { "bearerAuth": [] }]`,
meaning anonymous access is accepted and a bearer token is honoured.
JSON-RPC methods emit `x-authentication` with `"required": false`.
