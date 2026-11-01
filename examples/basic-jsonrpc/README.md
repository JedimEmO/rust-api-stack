# Basic JSON-RPC example

A task service built with `jsonrpc_service!`, split into a contract crate and a server.
It shows permission checks per method, authored OpenRPC examples,
the generated API explorer and Prometheus metrics.

- [`api/`](api/) declares the methods and the request and response types.
- [`service/`](service/) implements them and serves them over HTTP.

## Run

```bash
cargo run -p basic-jsonrpc-service --locked
```

The service listens on port 3000:

- JSON-RPC endpoint: `POST http://localhost:3000/rpc`
- API explorer: `http://localhost:3000/rpc/explorer`
- OpenRPC document: `http://localhost:3000/rpc/explorer/openrpc.json`
- Prometheus metrics: `http://localhost:3000/metrics`

See [service/README.md](service/README.md) for the demo credentials and `curl` requests.
[wasm-ui-demo](../wasm-ui-demo/) is a browser client for this service.
