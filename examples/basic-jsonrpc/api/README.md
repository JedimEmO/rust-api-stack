# Basic JSON-RPC API

The contract crate for the [basic JSON-RPC example](../README.md).
It defines the request and response types and calls `jsonrpc_service!`,
which generates the `MyServiceTrait` server trait, the `MyServiceBuilder` router builder,
the `MyServiceClient` client, the OpenRPC document and the explorer routes.

## Methods

The contract is in [src/lib.rs](src/lib.rs).

| Method | Access |
|---|---|
| `sign_in` | public |
| `sign_out` | any signed-in user |
| `delete_everything` | `admin` permission |
| `list_tasks`, `create_task`, `update_task`, `delete_task`, `get_task` | any signed-in user |
| `get_profile`, `update_profile` | any signed-in user |
| `get_dashboard_stats` | any signed-in user |

`create_task` and `get_task` carry authored examples.
They appear in the OpenRPC document and in the explorer:
a successful `create_task` call, the `1001` error the service returns for a duplicate title,
and a `get_task` lookup that returns `null`.

The server picks the mount path; [the service](../service/README.md) mounts it at `/rpc`.

## Features

- `server` generates the server trait, the router builder and the explorer routes.
- `client` generates the client with the default reqwest transport.
- With no features, the crate has only the shared types and the OpenRPC document functions.

## Checks

```bash
cargo check -p basic-jsonrpc-api --locked
cargo check -p basic-jsonrpc-api --features server --locked
cargo check -p basic-jsonrpc-api --features client --locked
cargo test -p basic-jsonrpc-api --locked
cargo test -p basic-jsonrpc-api --features server --locked
cargo test -p basic-jsonrpc-api --features client --locked
cargo clippy -p basic-jsonrpc-api --all-targets --all-features --locked -- -D warnings
```
