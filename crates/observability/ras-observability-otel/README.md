# ras-observability-otel

Request metrics for Rust Agent Stack services, recorded with OpenTelemetry and exported for Prometheus.
It implements the `ras-observability-core` traits
and serves the collected metrics from an axum `/metrics` route.

## Installation

```toml
[dependencies]
ras-observability-otel = "0.2"
ras-observability-core = "0.2" # brings the tracker traits into scope
```

## Quick start

```rust
use ras_observability_otel::standard_setup;

let otel = standard_setup("my-service")?;

let app = axum::Router::new()
    // .merge(your service routers)
    .merge(otel.metrics_router());
```

`standard_setup` is `OtelSetupBuilder::new(name).build()`.
Use the builder to pass in an existing `prometheus::Registry` with `with_prometheus_registry`.
Building installs the meter provider as the OpenTelemetry global,
so build one `OtelSetup` per process.

`OtelSetup` hands out three things:

- `usage_tracker()`, an `OtelUsageTracker` that logs the request and counts it as started.
- `method_duration_tracker()`, an `OtelMethodDurationTracker` that logs the duration,
  records it, and counts the request as completed.
- `metrics()`, the shared `OtelMetrics`, for recording through `ServiceMetrics` yourself.

## Wiring the trackers into a generated service

The generated builders take closures, not tracker objects,
so each closure builds a `RequestContext` and calls the tracker.
This is the shape for a `rest_service!` builder;
the REST hooks receive the HTTP method and the route path:

```rust
use ras_observability_core::{MethodDurationTracker, RequestContext, UsageTracker};
use ras_observability_otel::OtelSetupBuilder;

let otel = OtelSetupBuilder::new("my-service").build()?;

let router = MyServiceBuilder::new(MyServiceImpl::new())
    .with_usage_tracker({
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
    })
    .with_method_duration_tracker({
        let tracker = otel.method_duration_tracker();
        move |method, path, user, duration| {
            let context = RequestContext::rest(method, path);
            let tracker = tracker.clone();
            let user = user.cloned();
            async move {
                tracker.track_duration(&context, user.as_ref(), duration).await;
            }
        }
    })
    .build();
```

Notice that each closure sits inside the builder call.
Keep it there, or annotate the parameter types:
a closure bound to a `let` first, with unannotated reference parameters,
does not get the higher-ranked signature the builder needs.

A `jsonrpc_service!` builder takes the same two hooks.
Its usage tracker receives the `JsonRpcRequest`, so the context is `RequestContext::jsonrpc(request.method.clone())`,
and its duration tracker receives the method name.
[examples/basic-jsonrpc](https://github.com/JedimEmO/rust-api-stack/blob/master/examples/basic-jsonrpc/service/src/main.rs)
wires both.
The `file_service!` hooks are synchronous closures named `with_usage_tracker` and `with_duration_tracker`;
call `ServiceMetrics` on `otel.metrics()` from them.

## Metrics

| Prometheus name | Type | Labels |
|---|---|---|
| `requests_started_total` | counter | `method`, `protocol` |
| `requests_completed_total` | counter | `method`, `protocol`, `success` |
| `method_duration_milliseconds` | histogram | `method`, `protocol` |

The exporter also adds its own scope labels, such as `otel_scope_name`.
`protocol` is `REST`, `JSON-RPC` or `WebSocket`.

`OtelMethodDurationTracker` always records `success="true"`,
because the generated duration hooks do not say whether the handler failed.
To count failures, call `ServiceMetrics::increment_requests_completed` with `false` yourself.

User attributes never become metric labels; a label per user would create a series per user.
The trackers do log at `info` level, though,
and the usage tracker's log line includes the user ID and the full permission set.

## Examples

`examples/simple_usage.rs` records metrics by hand,
and `examples/with_rest_service.rs` builds the tracker closures next to a plain axum route.
Both listen on port 3000:

```bash
cargo run --example simple_usage -p ras-observability-otel --locked
curl http://localhost:3000/metrics
```

## Checks

```bash
cargo test -p ras-observability-otel --locked
cargo clippy -p ras-observability-otel --all-targets --all-features --locked -- -D warnings
```
