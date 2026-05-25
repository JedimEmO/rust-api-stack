# OAuth2 Demo API

Shared JSON-RPC API contract for the [OAuth2 demo](../README.md). This crate defines the service payloads and uses `ras-jsonrpc-macro` to generate the `GoogleOAuth2Service` server trait and OpenRPC document consumed by the demo server.

## Generated Service

The contract in [src/lib.rs](src/lib.rs) includes permission-gated methods for:

- current user information
- document listing
- document creation
- document deletion
- system status
- beta feature access

The runnable OAuth2 server is documented in [../server/README.md](../server/README.md).

## Permissions

The generated service metadata declares these permission checks:

- `user:read`
- `content:create`
- `admin:write`
- `system:admin`
- `beta:access`

The server decides how OAuth2 identities map to those permissions.

## Checks

```bash
cargo check -p oauth2-demo-api --locked
cargo check -p oauth2-demo-api --features server --locked
cargo check -p oauth2-demo-api --features client --locked
cargo test -p oauth2-demo-api --locked
cargo test -p oauth2-demo-api --features server --locked
cargo clippy -p oauth2-demo-api --all-targets --all-features --locked -- -D warnings
```
