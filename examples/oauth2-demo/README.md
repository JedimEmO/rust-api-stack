# OAuth2 demo

Sign in with Google through the authorization-code flow with PKCE,
turn the Google identity into a JWT session,
and call JSON-RPC methods that require different permissions.

- [`api/`](api/README.md) declares the JSON-RPC contract.
- [`server/`](server/README.md) serves the sign-in pages, the OAuth2 callback, the session service and the API.

## Run

You need a Google OAuth2 client; [server/README.md](server/README.md#google-cloud-setup) walks through creating one.
Then create the environment file and fill in your client id, client secret and a JWT secret:

```bash
cp examples/oauth2-demo/server/.env.example examples/oauth2-demo/server/.env
cargo run -p oauth2-demo-server --locked
```

The server reads `examples/oauth2-demo/server/.env` wherever you start it from,
and listens on `http://localhost:3000`.
Open that address and choose **Sign in with Google**.
