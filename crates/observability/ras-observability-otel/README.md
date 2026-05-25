# ras-observability-otel

OpenTelemetry implementation for Rust Agent Stack observability, providing runtime metrics collection with Prometheus export.

## Quick Start

```rust
use ras_observability_otel::standard_setup;

// Build the OpenTelemetry/Prometheus setup.
let otel = standard_setup("my-service")?;

// Use `otel.metrics_router()` for /metrics and wire the trackers into service
// builders with their observability hooks.
```

## Features

- **Convenience setup**: Sensible defaults with optional builder customization
- **Prometheus integration**: Built-in `/metrics` endpoint
- **Standard metrics**: Request counts and duration histograms
- **Axum integration**: Ready-to-use metrics router
- **Type-safe**: Leverages Rust's type system for safety

## Usage with Service Builders

The observability crates integrate with the REST and JSON-RPC macro builders:

```rust
use ras_observability_core::{RequestContext, UsageTracker};
use ras_observability_otel::OtelSetupBuilder;

// The service builders can use the trackers like this:
let otel = OtelSetupBuilder::new("my-service").build()?;

// REST service builders receive headers, user, method, and path.
let rest_usage_tracker = {
    let tracker = otel.usage_tracker();
    move |headers, user, method, path| {
        let context = RequestContext::rest(method, path);
        let tracker = tracker.clone();
        let headers = headers.clone();
        let user = user.cloned();
        async move {
            tracker.track_request(&headers, user.as_ref(), &context).await;
        }
    }
};

// REST service builders take the trait implementation.
MyServiceBuilder::new(MyServiceImpl::new())
    .with_usage_tracker(rest_usage_tracker)
    .build();

// JSON-RPC service builders receive headers, user, and the JSON-RPC request.
let rpc_usage_tracker = {
    let tracker = otel.usage_tracker();
    move |headers, user, request| {
        let context = RequestContext::jsonrpc(request.method.clone());
        let tracker = tracker.clone();
        let headers = headers.clone();
        let user = user.cloned();
        async move {
            tracker.track_request(&headers, user.as_ref(), &context).await;
        }
    }
};

// JSON-RPC service builders also take the trait implementation.
MyRpcServiceBuilder::new(MyRpcServiceImpl::new())
    .with_usage_tracker(rpc_usage_tracker)
    .build()?;
```

## Metrics Exposed

### Counters
- `requests_started_total`: Total requests initiated
- `requests_completed_total`: Total requests completed (with success status)

### Histograms
- `method_duration_milliseconds`: Method execution time in milliseconds (only includes method and protocol labels to avoid cardinality explosion)

### Labels
All metrics use minimal labels to prevent cardinality explosion:
- `method`: The method being called (e.g., "GET /users", "createUser")
- `protocol`: REST, JSON-RPC, or WebSocket
- `success`: "true" or "false" (only on completion counters)

**Note**: User attributes are intentionally excluded from all metrics to prevent cardinality explosion. User-specific analysis should be done through logs or dedicated user analytics systems.

## Examples

See the `examples/` directory for:
- `simple_usage.rs`: Basic metrics collection
- `with_rest_service.rs`: Integration with REST services

## Running Examples

```bash
# Simple usage example
cargo run --example simple_usage -p ras-observability-otel --locked

# Then visit http://localhost:3000/metrics
```

## Checks

```bash
cargo test -p ras-observability-otel --locked
cargo clippy -p ras-observability-otel --all-targets --all-features --locked -- -D warnings
```
