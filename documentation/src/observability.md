# Observability

RAS exposes observability hooks so generated services can report request usage
and method durations without baking one metrics backend into every macro.

The OpenTelemetry implementation lives in `ras-observability-otel`, and the
core traits live in `ras-observability-core`.

## Standard Setup

```rust,ignore
use ras_observability_otel::standard_setup;

let otel = standard_setup("my-service")?;
let usage_tracker = otel.usage_tracker();
let duration_tracker = otel.method_duration_tracker();
let metrics_router = otel.metrics_router();
```

Generated service builders expose hooks such as `with_usage_tracker` and
`with_method_duration_tracker` where supported:

```rust,ignore
let service = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
    .with_usage_tracker(usage_tracker)
    .with_method_duration_tracker(duration_tracker)
    .build();

let app = axum::Router::new()
    .merge(service)
    .merge(metrics_router);
```

## Request Contexts

Use standard context constructors when recording custom metrics outside a
generated service:

```rust,ignore
use ras_observability_core::RequestContext;

let rest = RequestContext::rest("POST", "/api/orders");
let rpc = RequestContext::jsonrpc("create_order");
let ws = RequestContext::websocket("send_message");
```

Consistent context names keep metrics comparable across REST, JSON-RPC, file,
and WebSocket APIs.

See
[crates/observability/ras-observability-otel](https://github.com/JedimEmO/rust-api-stack/tree/master/crates/observability/ras-observability-otel)
for crate-level details and examples.
