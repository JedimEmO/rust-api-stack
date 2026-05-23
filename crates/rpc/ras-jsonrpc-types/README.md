# ras-jsonrpc-types

Pure JSON-RPC 2.0 protocol types and utilities for Rust.

## Overview

This crate provides type-safe representations of JSON-RPC 2.0 protocol structures including requests, responses, and errors. It is designed to be lightweight and reusable across different JSON-RPC implementations.

## Features

- **JSON-RPC 2.0 compliant**: Full support for the JSON-RPC 2.0 specification
- **Type safe**: Strong typing with serde serialization/deserialization
- **Minimal dependencies**: Only depends on `serde` and `serde_json`
- **Standard error codes**: Predefined error codes following the JSON-RPC 2.0 spec
- **Convenience methods**: Helper methods for creating requests, responses, and errors

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
ras-jsonrpc-types = "0.1.1"
```

### Basic Types

```rust
use ras_jsonrpc_types::{JsonRpcRequest, JsonRpcResponse, JsonRpcError};

// Create a request
let request = JsonRpcRequest::new(
    "subtract".to_string(),
    Some(serde_json::json!([42, 23])),
    Some(serde_json::json!(1))
);

// Create a successful response
let response = JsonRpcResponse::success(
    serde_json::json!(19),
    Some(serde_json::json!(1))
);

// Create an error response
let error_response = JsonRpcResponse::error(
    JsonRpcError::method_not_found("unknown_method"),
    Some(serde_json::json!(1))
);
```

### Error Handling

```rust
use ras_jsonrpc_types::{JsonRpcError, error_codes};

// Standard JSON-RPC errors
let parse_error = JsonRpcError::parse_error();
let invalid_request = JsonRpcError::invalid_request();
let method_not_found = JsonRpcError::method_not_found("foo");
let invalid_params = JsonRpcError::invalid_params("Invalid parameters".to_string());
let internal_error = JsonRpcError::internal_error("Server error".to_string());

// Custom authentication errors
let auth_required = JsonRpcError::authentication_required();
let insufficient_perms = JsonRpcError::insufficient_permissions(
    vec!["admin".to_string()],
    vec!["user".to_string()]
);
let token_expired = JsonRpcError::token_expired();
```

## JSON-RPC 2.0 Specification

This crate provides the core [JSON-RPC 2.0 specification](https://www.jsonrpc.org/specification) request, response, and error types used by RAS services:

### Request Structure
```json
{
  "jsonrpc": "2.0",
  "method": "subtract",
  "params": [42, 23],
  "id": 1
}
```

### Response Structure
```json
{
  "jsonrpc": "2.0",
  "result": 19,
  "id": 1
}
```

### Error Response Structure
```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32601,
    "message": "Method not found"
  },
  "id": 1
}
```

## Error Codes

The crate provides all standard JSON-RPC 2.0 error codes plus extension codes for authentication:

| Code | Meaning |
|------|---------|
| -32700 | Parse error |
| -32600 | Invalid Request |
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |
| -32001 | Authentication required (extension) |
| -32002 | Insufficient permissions (extension) |
| -32003 | Token expired (extension) |

## Integration

This crate is designed to work with:

- [`ras-jsonrpc-core`](../ras-jsonrpc-core) - Authentication and authorization traits
- [`ras-jsonrpc-macro`](../ras-jsonrpc-macro) - Procedural macros for service generation
- Any JSON-RPC client or server implementation

## Checks

```bash
cargo test -p ras-jsonrpc-types --locked
cargo clippy -p ras-jsonrpc-types --all-targets --all-features --locked -- -D warnings
```

## License

This project is licensed under either MIT or Apache-2.0.
