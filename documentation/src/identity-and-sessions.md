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
placing bearer tokens in JavaScript. The same generated builders support cookie
auth transport and double-submit CSRF protection for unsafe cookie-authenticated
requests.

See the OAuth2 example in
[examples/oauth2-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/oauth2-demo).
