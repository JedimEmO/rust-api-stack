# Identity And Sessions

RAS separates service-level authorization from identity-provider concerns. The
service macros ask an `AuthProvider` to authenticate credentials and check
permissions. The identity crates help build those credentials and permission
sets.

## Core Pieces

- `ras-auth-core` defines `AuthProvider`, `AuthenticatedUser`, `AuthError`,
  bearer/cookie transport helpers, and CSRF configuration.
- `ras-identity-core` defines identity-provider traits.
- `ras-identity-local` provides username/password verification with Argon2.
- `ras-identity-oauth2` provides OAuth2 with PKCE support.
- `ras-identity-session` issues and verifies JWT sessions and can attach
  permissions to authenticated identities.

## Typical Flow

1. A public endpoint such as `sign_in` or an OAuth2 callback verifies an
   identity.
2. The application creates a JWT session through the session crate.
3. Protected generated services receive bearer tokens or configured secure
   cookies.
4. The generated service calls the configured `AuthProvider`.
5. Handler methods receive `&AuthenticatedUser` only after auth succeeds.

```rust,ignore
let jwt_auth = JwtAuthProvider::new(Arc::new(session_service));

let app = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(jwt_auth)
    .build();
```

## Permissions

Permissions are ordinary strings stored on `AuthenticatedUser`. The default
`AuthProvider::check_permissions` requires all permissions in a group. Override
it when permissions are tenant-aware, role-derived, time-bound, or backed by an
external policy service.

Use `WITH_PERMISSIONS([])` when an operation only needs a logged-in user and no
specific permission.

## Secure Browser Sessions

Browser-facing services can use secure `HttpOnly` cookies instead of manually
placing bearer tokens in JavaScript. Cookie auth is not two independent knobs:
because the browser attaches cookies automatically, cookie credentials are
**always** paired with CSRF protection. Calling `.auth_cookie(...)` installs a
default double-submit `CsrfConfig` for you, and a transport that enables cookies
without a CSRF config fails to `build()`. Override the default with
`.csrf_protection(...)` if you need a session-bound token or a custom header, but
there is deliberately no builder path to cookie auth without CSRF.

CSRF is enforced only for cookie credentials on unsafe methods (`POST`, `PUT`,
`PATCH`, `DELETE`). Bearer tokens and safe methods remain exempt.

See the OAuth2 example in
[examples/oauth2-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/oauth2-demo).
