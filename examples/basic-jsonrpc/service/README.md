# Basic JSON-RPC service

The runnable server for the [basic JSON-RPC example](../README.md).
It implements the `MyService` contract from [`basic-jsonrpc-api`](../api/README.md),
keeps tasks in memory, and records request metrics through `ras-observability-otel`.

## Run

```bash
cargo run -p basic-jsonrpc-service --locked
```

The service listens on `0.0.0.0:3000`:

- JSON-RPC endpoint: `POST http://localhost:3000/rpc`
- API explorer: `http://localhost:3000/rpc/explorer`
- OpenRPC document: `http://localhost:3000/rpc/explorer/openrpc.json`
- Prometheus metrics: `http://localhost:3000/metrics`

Tasks live in memory and are gone when the process stops.

## Credentials

Authentication is a fixed demo: `sign_in` checks two hard-coded accounts
and returns a fixed bearer token instead of a real JWT.

| Username | Password | Token | Permissions |
|---|---|---|---|
| `user` | `password` | `valid_token` | `user` |
| `admin` | `secret` | `admin_token` | `user`, `admin` |

## Sign in

```bash
curl -X POST http://localhost:3000/rpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "sign_in",
    "params": {
      "WithCredentials": {
        "username": "admin",
        "password": "secret"
      }
    },
    "id": 1
  }'
```

The response carries the token:

```json
{
  "jsonrpc": "2.0",
  "result": {
    "Success": {
      "jwt": "admin_token"
    }
  },
  "id": 1
}
```

## Call a protected method

Methods whose request type is `()` take no `params`.
Leave the field out, or send `null`; an empty object is rejected as invalid params.

```bash
curl -X POST http://localhost:3000/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer admin_token" \
  -d '{
    "jsonrpc": "2.0",
    "method": "delete_everything",
    "id": 2
  }'
```

Create a task with the user token:

```bash
curl -X POST http://localhost:3000/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer valid_token" \
  -d '{
    "jsonrpc": "2.0",
    "method": "create_task",
    "params": {
      "title": "Ship the example",
      "description": "Exercise the generated client",
      "priority": "High"
    },
    "id": 3
  }'
```

## Metrics

The service passes the trackers from `ras-observability-otel`
to the generated builder with `with_usage_tracker` and `with_method_duration_tracker`.
The Prometheus endpoint exposes three metrics:

- `requests_started_total`, labelled `method` and `protocol`
- `requests_completed_total`, labelled `method`, `protocol` and `success`
- `method_duration_milliseconds`, a histogram labelled `method` and `protocol`

`success` is `false` when the call ended in a JSON-RPC error,
such as invalid params or a handler error.
Calls rejected by authentication or authorization, and calls to unknown methods,
are not counted at all; they are logged at `warn` instead.

User ids go to the `tracing` log, not into metric labels,
so the number of time series does not grow with the number of users.

```bash
curl http://localhost:3000/metrics
```

After one `sign_in` call, the output contains lines of this shape:

```text
requests_started_total{method="sign_in",protocol="JSON-RPC"} 1
requests_completed_total{method="sign_in",protocol="JSON-RPC",success="true"} 1
method_duration_milliseconds_bucket{method="sign_in",protocol="JSON-RPC",le="5"} 1
```

The service does not export OTLP itself.
To send the metrics to an OTLP backend, run an OpenTelemetry Collector
that scrapes `/metrics` and forwards them:

```yaml
receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: 'basic-jsonrpc-service'
          static_configs:
            - targets: ['host.docker.internal:3000']

exporters:
  otlp:
    endpoint: "your-otlp-backend:4317"

service:
  pipelines:
    metrics:
      receivers: [prometheus]
      exporters: [otlp]
```

## Checks

The unit tests cover the demo authentication, the task lifecycle, the profile methods and missing-task errors.

```bash
cargo test -p basic-jsonrpc-service --locked
cargo clippy -p basic-jsonrpc-service --all-targets --all-features --locked -- -D warnings
```
