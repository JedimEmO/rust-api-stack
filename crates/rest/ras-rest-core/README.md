# ras-rest-core

Runtime types shared by Rust Agent Stack REST services.

This crate is intentionally small. It provides the response and error types
generated REST handlers use, re-exports the shared authentication types, and
exposes the version migration trait used by versioned REST endpoints.

## Key Types

- `RestResponse<T>` wraps a response body with an explicit HTTP status.
- `RestResult<T>` is the standard result returned by REST handlers.
- `RestError` carries a client-safe status and message plus optional internal
  error details for logging.
- `RestResultExt` converts ordinary `Result<T, E>` values into REST results.

## Example

```rust
use ras_rest_core::{RestError, RestResponse, RestResult};

async fn get_user(id: u64) -> RestResult<String> {
    if id == 0 {
        return Err(RestError::bad_request("user id must be non-zero"));
    }

    Ok(RestResponse::ok(format!("user-{id}")))
}
```

## Checks

```bash
cargo test -p ras-rest-core --locked
cargo clippy -p ras-rest-core --all-targets --all-features --locked -- -D warnings
```
