# Observability

The generated services expose hooks that run before and after each request,
so you can count requests and time handlers without the macros depending on a metrics backend.
`ras-observability-core` defines the backend-neutral traits,
and `ras-observability-otel` implements them with OpenTelemetry and a Prometheus `/metrics` route.

## Standard setup

```rust,ignore
use ras_observability_otel::standard_setup;

let otel = standard_setup("my-service")?;
let metrics_router = otel.metrics_router();
```

`standard_setup` installs its meter provider as the OpenTelemetry global,
so call it once per process.

## Wiring the hooks

The builders take closures, not tracker objects.
Each closure builds a `RequestContext` and calls the tracker from `otel`.
For a `rest_service!` builder, the hooks receive the HTTP method and the route path:

```rust,ignore
use ras_observability_core::{MethodDurationTracker, RequestContext, UsageTracker};

let service = UserServiceBuilder::new(UserServiceImpl)
    .auth_provider(my_auth_provider)
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

let app = axum::Router::new()
    .merge(service)
    .merge(metrics_router);
```

The hooks differ per macro:

| Macro | Usage hook | Duration hook |
|---|---|---|
| `rest_service!` | `with_usage_tracker(headers, user, method, path)`, async | `with_method_duration_tracker(method, path, user, duration)`, async |
| `jsonrpc_service!` | `with_usage_tracker(headers, user, request)`, async | `with_method_duration_tracker(method, user, duration)`, async |
| `file_service!` | `with_usage_tracker(headers, method, path)`, sync | `with_duration_tracker(method, path, duration)`, sync |
| `jsonrpc_bidirectional_service!` | none | none |

For a JSON-RPC service, build the context with `RequestContext::jsonrpc(request.method.clone())`.
The file service hooks are synchronous,
so call the `ServiceMetrics` methods on `otel.metrics()` from them.
A bidirectional service has no hooks; record its messages yourself with `RequestContext::websocket`.

The usage hook sees the request headers with credential headers redacted.
`OtelMethodDurationTracker` counts every completed request as `success="true"`,
because the duration hooks do not say whether the handler failed.

## Request contexts

Use the same constructors when you record metrics outside a generated service:

```rust,ignore
use ras_observability_core::RequestContext;

let rest = RequestContext::rest("POST", "/api/orders");
let rpc = RequestContext::jsonrpc("create_order".to_string());
let ws = RequestContext::websocket("send_message");
```

The context's `method` becomes a metric label,
so keep it to a bounded set of names, such as route templates rather than paths with IDs in them.

See the
[`ras-observability-otel` README](https://github.com/JedimEmO/rust-api-stack/tree/master/crates/observability/ras-observability-otel)
for the exported metric names and the examples.
