# OAuth2 demo API

The contract crate for the [OAuth2 demo](../README.md).
It defines the request and response types and calls `jsonrpc_service!`,
which generates the `GoogleOAuth2Service` server trait, a Rust client and the OpenRPC document.

## Methods

The contract is in [src/lib.rs](src/lib.rs). Each method requires a signed-in user and, for most, one permission:

| Method | Permission |
|---|---|
| `get_user_info` | none beyond signing in |
| `list_documents` | `user:read` |
| `create_document` | `content:create` |
| `delete_document` | `admin:write` |
| `get_system_status` | `system:admin` |
| `get_beta_features` | `beta:access` |

The contract only names the permissions.
The [server](../server/README.md#permission-rules) decides which Google users hold them.

## Features

The service uses `feature_gated: true`, so this crate's features choose what is generated:

- `server` generates the server trait and the router builder.
- `client` generates the Rust client with the default reqwest transport.

## Checks

```bash
cargo check -p oauth2-demo-api --locked
cargo check -p oauth2-demo-api --features server --locked
cargo check -p oauth2-demo-api --features client --locked
cargo test -p oauth2-demo-api --locked
cargo test -p oauth2-demo-api --features server --locked
cargo clippy -p oauth2-demo-api --all-targets --all-features --locked -- -D warnings
```
