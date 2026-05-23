# OAuth2 Demo

This example demonstrates a Google OAuth2 authorization-code flow with PKCE,
JWT session creation, and permission-protected JSON-RPC methods.

## Crates

- `api/` defines the JSON-RPC contract shared by the server.
- `server/` hosts the OAuth2 callback, static pages, session service, and API.

## Configure

Create the server-local environment file:

```bash
cp examples/oauth2-demo/server/.env.example examples/oauth2-demo/server/.env
```

Edit `examples/oauth2-demo/server/.env` with your Google OAuth2 client ID,
client secret, redirect URI, and JWT secret.

## Run

From the workspace root:

```bash
cargo run -p oauth2-demo-server --locked
```

The server loads `examples/oauth2-demo/server/.env` when run by package name and
starts at `http://localhost:3000`.

See [server/README.md](server/README.md) for Google Cloud setup and API
usage examples.
