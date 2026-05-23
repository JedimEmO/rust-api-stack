# Basic JSON-RPC Service

Runnable JSON-RPC task service demonstrating the `jsonrpc_service!` macro,
authentication, generated OpenRPC documentation, and Prometheus-compatible
metrics.

## Run

From the workspace root:

```bash
cargo run -p basic-jsonrpc-service --locked
```

The service listens on `http://localhost:3000`:

- JSON-RPC endpoint: `POST /rpc`
- Explorer UI: `/rpc/explorer`
- OpenRPC document: `/rpc/explorer/openrpc.json`
- Prometheus metrics: `/metrics`

## Credentials

The example uses fixed demo credentials:

- User: `user` / `password`, returns bearer token `valid_token`
- Admin: `admin` / `secret`, returns bearer token `admin_token`

## Sign In

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

Successful admin response:

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

## Authenticated Request

```bash
curl -X POST http://localhost:3000/rpc \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer admin_token" \
  -d '{
    "jsonrpc": "2.0",
    "method": "delete_everything",
    "params": {},
    "id": 2
  }'
```

## Metrics

The service wires `ras-observability-otel` into the generated JSON-RPC builder
with `with_usage_tracker` and `with_method_duration_tracker`.

Prometheus metrics use low-cardinality labels:

- `requests_started_total`: labels `method`, `protocol`
- `requests_completed_total`: labels `method`, `protocol`, `success`
- `method_duration_milliseconds`: labels `method`, `protocol`

The trackers log authenticated user details with `tracing`, but the metrics do
not include user ids, session ids, request ids, or arbitrary path values.

Check metrics after making a request:

```bash
curl http://localhost:3000/metrics
```

Example output shape:

```text
requests_started_total{method="sign_in",protocol="JSON-RPC"} 1
requests_completed_total{method="sign_in",protocol="JSON-RPC",success="true"} 1
method_duration_milliseconds_bucket{method="sign_in",protocol="JSON-RPC",le="5"} 1
```

## OpenTelemetry Collector

This example exposes Prometheus text metrics. To forward them to an OTLP backend,
run an OpenTelemetry Collector that scrapes `http://localhost:3000/metrics` and
exports to your OTLP destination.

Minimal collector sketch:

```yaml
receivers:
  prometheus:
    config:
      scrape_configs:
        - job_name: 'basic-jsonrpc-service'
          static_configs:
            - targets: ['host.docker.internal:3000']

processors:
  batch:

exporters:
  otlp:
    endpoint: "tempo:4317"

service:
  pipelines:
    metrics:
      receivers: [prometheus]
      processors: [batch]
      exporters: [otlp]
```

## Tests

The service has focused unit tests for authentication, task lifecycle behavior,
profile methods, and missing-task errors:

```bash
cargo test -p basic-jsonrpc-service --locked
```

## Checks

```bash
cargo test -p basic-jsonrpc-service --locked
cargo clippy -p basic-jsonrpc-service --all-targets --all-features --locked -- -D warnings
```
