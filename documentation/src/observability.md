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
        move |method, path, user, duration, success| {
            let context = RequestContext::rest(method, path);
            let tracker = tracker.clone();
            let user = user.cloned();
            async move {
                tracker
                    .track_duration(&context, user.as_ref(), duration, success)
                    .await;
            }
        }
    })
    .build();

let app = axum::Router::new()
    .merge(service)
    .merge(metrics_router);
```

The hooks take the same shape in every macro;
only the arguments that name the request differ:

| Macro | Usage hook | Duration hook |
|---|---|---|
| `rest_service!` | `with_usage_tracker(headers, user, method, path)` | `with_method_duration_tracker(method, path, user, duration, success)` |
| `jsonrpc_service!` | `with_usage_tracker(headers, user, request)` | `with_method_duration_tracker(method, user, duration, success)` |
| `file_service!` | `with_usage_tracker(headers, user, method, path)` | `with_method_duration_tracker(method, path, user, duration, success)` |
| `jsonrpc_bidirectional_service!` | none | none |

Every hook returns a future, which the service awaits.
`path` is the route template relative to the service's base path, such as `/items/{id}`,
so it is safe to use as a metric label.
For a JSON-RPC service, build the context with `RequestContext::jsonrpc(request.method.clone())` in the usage hook;
the duration hook receives the method's wire name.
A bidirectional service has no hooks; record its messages yourself with `RequestContext::websocket`.

## When the hooks run

Both hooks run only for requests that pass authentication and authorization.
A request with a missing or invalid credential, or without a required permission,
reaches neither hook; the service logs the rejection at `warn` instead.
A JSON-RPC call to a method the service does not declare reaches neither hook either,
so a caller cannot grow your metric labels by inventing method names.

Once a request passes authorization,
the usage hook runs, then the request is decoded and handled,
and the duration hook runs exactly once with the finished response.
So `requests_started` and `requests_completed` count the same requests.
`duration` is the time from the usage hook to the finished response.
For a file download it ends when the response starts streaming, not when the last byte is sent.

`success` is `false` when the request ended in an error:

- for REST and file services, any response with a 4xx or 5xx status,
  such as a body or multipart part that does not decode, or a handler error;
- for JSON-RPC, any error response,
  such as invalid params, a handler error, or a result that does not serialize.

`OtelMethodDurationTracker` records it as the `success` label on `requests_completed`.
The usage hook sees the request headers with credential headers redacted.

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
