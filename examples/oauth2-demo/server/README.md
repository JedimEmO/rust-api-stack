# OAuth2 demo server

The runnable server for the [OAuth2 demo](../README.md).
It signs users in with Google, issues a JWT session through `ras-identity-session`,
and serves the `GoogleOAuth2Service` JSON-RPC API from [`oauth2-demo-api`](../api/README.md),
where each method requires a permission derived from the Google identity.

The pieces:

- `OAuth2Provider` from `ras-identity-oauth2` runs the authorization-code flow with PKCE.
  An in-memory state store and an `HttpOnly` binding cookie protect the callback against login CSRF.
- `SessionService` turns the verified identity into a JWT and keeps the active sessions in memory.
- `GoogleOAuth2Permissions` in [src/permissions.rs](src/permissions.rs) decides the permissions.
- `JwtAuthProvider` checks the bearer token on each JSON-RPC call.

## Google Cloud setup

1. Open the [Google Cloud Console](https://console.cloud.google.com/) and create or pick a project.
2. Configure the OAuth consent screen.
3. Under **Credentials**, create an **OAuth client ID** of type **Web application**.
4. Add `http://localhost:3000/auth/callback` as an authorized redirect URI.

## Configuration

Copy [.env.example](.env.example) to `.env` in this directory and fill it in.
The server loads this file even when started from the workspace root.

| Variable | Required | Default |
|---|---|---|
| `GOOGLE_CLIENT_ID` | yes | |
| `GOOGLE_CLIENT_SECRET` | yes | |
| `JWT_SECRET` | yes | |
| `REDIRECT_URI` | no | `http://localhost:3000/auth/callback` |
| `SERVER_HOST` | no | `0.0.0.0` |
| `SERVER_PORT` | no | `3000` |

`JWT_SECRET` must be at least 32 bytes and must not contain placeholder words such as `secret` or `example`.
Generate one with `openssl rand -hex 32`.

## Run

```bash
cargo run -p oauth2-demo-server --locked
```

1. Open `http://localhost:3000` and choose **Sign in with Google**.
2. After Google redirects back, the success page reads the JWT from the URL fragment,
   keeps it in `sessionStorage` and shows it.
3. Copy the token and set `JWT_TOKEN` to it to call the API.

The build script writes the OpenRPC document to `target/openrpc/googleoauth2service.json`
and the permission manifest to `target/ras-permissions/oauth2-demo.json`, both under this directory.

## Calling the API

The JSON-RPC endpoint is `POST http://localhost:3000/api/rpc`.
The page at `/api-docs` describes the methods.

```bash
curl -X POST http://localhost:3000/api/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $JWT_TOKEN" \
  -d '{"jsonrpc": "2.0", "method": "get_user_info", "params": {}, "id": 1}'
```

`get_user_info` returns your user id and permissions, which tells you which of the other methods you may call.

| Method | Permission | Example `params` |
|---|---|---|
| `get_user_info` | none | `{}` |
| `list_documents` | `user:read` | `{"limit": 10}` |
| `create_document` | `content:create` | `{"title": "Notes", "content": "Hello", "tags": ["demo"]}` |
| `delete_document` | `admin:write` | `{"document_id": "doc_123"}` |
| `get_system_status` | `system:admin` | `{}` |
| `get_beta_features` | `beta:access` | `{}` |

## Permission rules

`GoogleOAuth2Permissions` grants:

| Who | Permissions added |
|---|---|
| every signed-in user | `user:read`, `profile:read` |
| a verified email | `email:verified` |
| a verified email ending in `@trusted-domain.com` | `user:write`, `content:create`, `content:edit` |
| a verified email ending in `@example.com` | `admin:read`, `admin:write`, `user:write`, `system:manage` |
| a `locale` claim starting with `en` | `content:english` |
| the hard-coded subject `104872499792737890123` | `system:admin`, `debug:access` |
| a subject starting with `beta_` | `beta:access`, `feature:preview` |

Domain rules apply only when Google reports the email as verified,
because an unverified address proves nothing about who owns it.
A normal Google account gets only the first rows,
so to try the other methods, edit the rules in [src/permissions.rs](src/permissions.rs).

## Before you build on this

This is a local demo.
It runs over plain HTTP, so the binding cookie has no `Secure` flag,
and the OAuth2 state and the sessions live in process memory.
A real deployment needs HTTPS, shared storage for state and sessions,
and permission rules that fit its own users.

## Checks

```bash
cargo test -p oauth2-demo-server --locked
cargo clippy -p oauth2-demo-server --all-targets --all-features --locked -- -D warnings
```
