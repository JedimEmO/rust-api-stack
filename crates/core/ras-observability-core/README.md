# ras-observability-core

Backend-neutral traits and types for request metrics in Rust API Stack services.
They describe a request the same way whether it came through REST, JSON-RPC or a WebSocket,
so one metrics backend can serve all three.

This crate holds no metrics backend.
`ras-observability-otel` implements the traits with OpenTelemetry and a Prometheus `/metrics` endpoint;
use that crate unless you are writing your own backend.

## Installation

```toml
[dependencies]
ras-observability-core = "0.2"
```

## Request context

`RequestContext` names the operation and its protocol:

```rust
use ras_observability_core::RequestContext;

let rest = RequestContext::rest("GET", "/api/v1/users"); // method "GET /api/v1/users"
let rpc = RequestContext::jsonrpc("getUser".to_string());
let ws = RequestContext::websocket("send_message");

let rest = rest.with_metadata("request_id", "12345");
```

The `method` and `protocol` fields become metric labels in `ras-observability-otel`.
Keep `method` to a bounded set of values, such as a route template rather than a path with IDs in it,
or the number of metric series grows with your traffic.

## Traits

- `UsageTracker::track_request` runs before a request is handled,
  with the request headers, the authenticated user if any, and the context.
- `MethodDurationTracker::track_duration` runs after the handler returns, with the elapsed time.
- `ServiceMetrics` is the synchronous counter and histogram interface:
  requests started, requests completed with a success flag, and method duration.

The `extractors` module has two helpers, `user_agent` and `user_attributes`.
`user_attributes` returns the user ID and the full permission list,
so keep its output out of metric labels.

`ObservabilityBuilder` and `ObservabilityConfig` hold a usage tracker and a duration tracker as boxed closures.
The generated service builders do not take them; they have their own `with_usage_tracker` methods.

## Checks

```bash
cargo test -p ras-observability-core --locked
cargo clippy -p ras-observability-core --all-targets --all-features --locked -- -D warnings
```
