# Identity and sessions

A generated service only needs to answer one question per request:
which user does this token belong to, and what may they do?
It asks an `AuthProvider` from `ras-auth-core`.
The identity crates answer the questions before that one:
how a user proves who they are, and how that proof becomes a token.

## The crates

- `ras-auth-core` defines `AuthProvider`, `AuthenticatedUser`, `AuthError`, `Caller`,
  the bearer and cookie transport helpers, and `CsrfConfig`.
- `ras-identity-core` defines `IdentityProvider`, which verifies a login,
  and `UserPermissions`, which maps a verified identity to permissions.
- `ras-identity-local` verifies usernames and passwords against Argon2id hashes held in memory.
- `ras-identity-oauth2` runs the OAuth2 authorization-code flow with PKCE, state and login-CSRF binding.
- `ras-identity-session` issues and verifies HMAC-signed JWTs,
  and its `JwtAuthProvider` implements `AuthProvider` on top of them.

## Typical flow

1. A public endpoint, such as `sign_in` or an OAuth2 callback,
   passes the login payload to `SessionService::begin_session`.
2. The session service asks the registered `IdentityProvider` to verify it,
   asks the `UserPermissions` source for permissions, and returns a signed JWT.
3. The client sends the JWT as a bearer token or in a secure cookie.
4. The generated service passes it to the configured `AuthProvider`,
   which is `JwtAuthProvider` in this setup.
5. A `WITH_PERMISSIONS` handler runs only after authentication and the permission check succeed,
   and receives `&AuthenticatedUser`.
   An `OPTIONAL_AUTH` handler always runs, and receives a `Caller`.

```rust,ignore
let session_service = Arc::new(SessionService::new(session_config)?);
let jwt_auth = JwtAuthProvider::new(session_service.clone());

let app = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(jwt_auth)
    .build();
```

## User IDs

A subject is only unique within the identity provider that verified it,
so the session service puts the provider ID in front of it.
The user ID is `"<provider_id>:<subject>"`,
such as `local:alice` or `oauth2:google:1234`.
It is the JWT `sub` claim, and the `user_id` your handlers receive on `AuthenticatedUser`.
Key ownership and per-user data on it, never on the bare subject or username;
otherwise a user at one provider can reach the data of a namesake at another.

Registered provider IDs cannot contain `:`,
and a provider can only issue identities under its own ID,
so the text before the first `:` always names the provider that verified the login.
To give one person the same account across several providers,
map these user IDs to your own account IDs in the application.

## Permissions at login

The permissions are copied into the JWT when the session begins.
They are not reloaded on each request,
so a permission change reaches a user at their next login,
or sooner if you end their session.

## Sessions and revocation

`SessionConfig::new(secret, issuer, audience)` requires a secret of at least 32 bytes,
and rejects secrets that look like placeholders.
Tokens expire after 24 hours unless you set `jwt_ttl`.

By default `enforce_active_sessions` is on:
the service records each session's `jti` in memory,
and `verify_session` rejects a token whose `jti` is not there.
That is what makes `end_session(jti)` revoke a token at once.
It also means sessions belong to one process.
A restart logs every user out,
and a second instance rejects tokens the first one issued.
Turn the setting off to accept any valid, unexpired token on any instance,
at the cost of revocation: a token then stays valid until it expires.

## Permissions

Permissions are plain strings stored on `AuthenticatedUser`.
A `WITH_PERMISSIONS` requirement is a list of groups;
the user must hold every permission in at least one group.
The default `AuthProvider::check_permissions` does the per-group check.
Override it when permissions are tenant-aware, role-derived, time-bound,
or held by an external policy service.

Use `WITH_PERMISSIONS([])` when an operation needs a signed-in user and no specific permission.

## Browser sessions

A browser client can carry the JWT in a secure `HttpOnly` cookie
instead of keeping a bearer token where JavaScript can read it.
The browser attaches that cookie to cross-site requests too,
so cookie auth always comes with CSRF protection.
Calling `.auth_cookie(...)` on a generated builder installs a default double-submit `CsrfConfig`,
and `build()` refuses a transport that has cookies without a CSRF config:
the REST and file builders panic, and the JSON-RPC builder returns an error.
Override the default with `.csrf_protection(...)` to use a custom header or cookie name.

`CsrfConfig` also has two weaker modes,
`dangerous_header_presence_only(...)` and `dangerous_static_value(...)`.
Neither binds the CSRF token to the session,
and pairing either with cookie auth logs a `tracing::warn!`.
Prefer the default double-submit cookie.

CSRF is enforced only for cookie credentials on unsafe methods (`POST`, `PUT`, `PATCH`, `DELETE`).
Bearer tokens and safe methods are exempt.

The [OAuth2 example](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/oauth2-demo)
puts these pieces together with Google sign-in.
