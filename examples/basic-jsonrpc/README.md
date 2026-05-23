# Basic JSON-RPC Example

This example is a two-crate JSON-RPC service that demonstrates shared API
definitions, authentication, generated OpenRPC documentation, and Prometheus
metrics.

## Crates

- `api/` defines the JSON-RPC methods and shared request/response types.
- `service/` implements the handlers, authentication, metrics, and HTTP server.

## Run

From the workspace root:

```bash
cargo run -p basic-jsonrpc-service --locked
```

The service starts at `http://localhost:3000` with:

- JSON-RPC endpoint: `POST /rpc`
- Explorer UI: `/rpc/explorer`
- OpenRPC document: `/rpc/explorer/openrpc.json`
- Prometheus metrics: `/metrics`

See [service/README.md](service/README.md) for credentials and request examples.
