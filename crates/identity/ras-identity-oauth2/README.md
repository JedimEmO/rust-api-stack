# ras-identity-oauth2

An OAuth2 sign-in provider for Rust API Stack services.
It runs the authorization-code flow with PKCE, checks the OpenID Connect `id_token` when one is returned,
and hands the user's identity to `ras-identity-session`, which issues a JWT.

One `OAuth2Provider` can hold several configured providers, such as Google and a company IdP.
It registers with the session service under the single ID `"oauth2"`.

## Installation

```toml
[dependencies]
ras-identity-oauth2 = "0.3"
ras-identity-session = "0.4"
```

## Configuring a provider

```rust
use ras_identity_oauth2::{InMemoryStateStore, OAuth2Config, OAuth2Provider, OAuth2ProviderConfig};
use std::{collections::HashMap, env, sync::Arc};

let google = OAuth2ProviderConfig {
    provider_id: "google".to_string(),
    client_id: env::var("GOOGLE_CLIENT_ID")?,
    client_secret: env::var("GOOGLE_CLIENT_SECRET")?,
    authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
    token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
    userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v3/userinfo".to_string()),
    // Required whenever the token endpoint returns an id_token, as it does with `openid`.
    issuer: Some("https://accounts.google.com".to_string()),
    redirect_uri: "https://app.example.com/auth/callback".to_string(),
    scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
    auth_params: HashMap::new(),
    use_pkce: true,
    user_info_mapping: None,
    metadata_claims: Vec::new(),
    allow_insecure_endpoints: false,
};

let config = OAuth2Config::new()
    .add_provider(google)
    .with_state_ttl(600) // seconds; the default
    .with_http_timeout(30); // seconds; the default

let oauth2_provider = OAuth2Provider::new(config, Arc::new(InMemoryStateStore::new()));
```

`OAuth2Provider::new` panics on an invalid configuration;
`OAuth2Provider::try_new` returns the error instead.

## Running the flow

Register a clone of the provider with the session service, and keep the original to start flows.
`IdentityProvider::verify` only completes a flow; starting one goes through `start_flow`.

```rust
use ras_identity_oauth2::{OAuth2AuthPayload, OAuth2Response};
use ras_identity_session::{SessionConfig, SessionService};

let session_config = SessionConfig::new(std::env::var("JWT_SECRET")?, "my-service", "my-service")?;
let session_service = SessionService::new(session_config)?;
session_service.register_provider(Box::new(oauth2_provider.clone())).await;

// 1. Start the flow.
if let OAuth2Response::AuthorizationUrl { url, binding, .. } =
    oauth2_provider.start_flow("google", None).await?
{
    // Set `binding` in an HttpOnly, Secure, SameSite=Lax cookie, then redirect to `url`.
}

// 2. On the callback, pass the query parameters and the binding cookie back in.
//    `query` holds the callback's query string; `binding_from_cookie` is an Option<String>.
let payload = OAuth2AuthPayload::Callback {
    provider_id: "google".to_string(),
    code: query.code,
    state: query.state,
    error: query.error,
    error_description: query.error_description,
    binding: binding_from_cookie,
};
let jwt = session_service
    .begin_session("oauth2", serde_json::to_value(payload)?)
    .await?;
```

The binding cookie needs `SameSite=Lax`, not `Strict`:
the callback is a top-level redirect from the identity provider's site,
and the browser leaves `Strict` cookies off that request.

`start_flow` always generates a binding.
For a non-browser flow where login CSRF does not apply,
`start_flow_bound(provider_id, params, None)` starts an unbound flow.
`start_flow_bound` also accepts a binding value of your own.

`OAuth2Client` is the lower layer under `OAuth2Provider`,
for callers who want the `TokenResponse` itself instead of a `VerifiedIdentity`.
It follows the same rules:
`generate_authorization_url` returns a binding for the callback,
`generate_authorization_url_bound` takes your own or `None`,
and every method checks the provider config with `validate` before using it.

A complete server, with the binding cookie, is in
[examples/oauth2-demo](https://github.com/JedimEmO/rust-api-stack/tree/master/examples/oauth2-demo).

## What the callback checks

The callback fails unless all of these hold:

- **State.** The `state` parameter names a stored flow that has not expired
  and belongs to the same provider.
  The state is a random UUID, and the in-memory store deletes it on first lookup,
  so a state works once, even when a later check fails.
- **Binding.** The callback's `binding` equals the value stored at the start, compared in constant time.
  This stops login CSRF, where an attacker gets a victim to finish the attacker's flow.
- **PKCE.** With `use_pkce`, the token request carries the verifier for the `S256` challenge sent at the start.
- **The id_token**, when the token endpoint returns one:
  `iss` equals the configured `issuer`, and a missing `issuer` rejects the token;
  `aud` contains the `client_id`, with `azp` equal to it when there are several audiences;
  `exp` is in the future;
  `nonce` equals the one sent at the start;
  and `sub` is present and equals the userinfo `sub`.
  The signature is not verified.
  OIDC Core section 3.1.3.7 allows that in the code flow,
  because the token comes straight from the token endpoint over TLS.

A callback that carries an `error` from the provider fails with that error code;
the `error_description` goes to the server log, not to the client.

## Other protections

- Authorization, token and userinfo endpoints must use `https://`,
  unless `allow_insecure_endpoints` is set for a local mock IdP.
  `redirect_uri` is not checked.
- `auth_params` and per-flow parameters cannot override the parameters the crate sets,
  such as `redirect_uri`, `state`, `nonce`, `scope`, the PKCE fields, `request` and `response_mode`.
- Only `picture`, `email_verified` and the claims listed in `metadata_claims` reach the identity's metadata,
  and from there the session JWT.
- Token and userinfo requests do not follow redirects, and a `3xx` answer fails the request.
  Following one would replay the token request, `client_secret` and code included, to the new location.
- `client_secret` is skipped when the config is serialized, and redacted in its `Debug` output.
  `Debug` also redacts the tokens in `TokenResponse`,
  the authorization code, the PKCE verifier and the login-CSRF binding,
  wherever they appear in the crate's types.
- Token and userinfo error responses are logged by status code only; their bodies may contain tokens.

## The resulting identity

The `VerifiedIdentity` has `provider_id` `"oauth2:<provider_id>"`, such as `"oauth2:google"`.
Its `subject`, email, display name and picture come from the userinfo response.

The userinfo endpoint is called on every callback,
so `userinfo_endpoint` must be set even though the field is an `Option`.
The response must carry the user ID as a JSON string in `sub` or `id`.

`user_info_mapping` reads the subject, email, name and picture from other claim names.
A mapped field falls back to the standard claim when the mapped claim is missing.

## Provider compatibility

The token endpoint must answer with JSON;
the crate does not send an `Accept: application/json` header.
Google and other OpenID Connect providers work.
GitHub does not work as is:
its token endpoint answers with a form-encoded body unless asked for JSON,
and its user API returns a numeric `id`.

## State storage

`InMemoryStateStore` holds up to 10,000 pending flows.
When it is full, it drops the flow closest to expiry, so a burst of new flows can push out a real user's flow.
Rate-limit flow starts in front of the service.
The store lives in one process,
so a deployment with several instances needs a shared store behind the `OAuth2StateStore` trait.

A custom store must keep the guarantees the in-memory one gives:
`retrieve` removes the state atomically, so it can be used once,
and returns `OAuth2Error::StateNotFound` for an expired state.
The client does not check expiry again.

```rust
use async_trait::async_trait;
use ras_identity_oauth2::{OAuth2Result, OAuth2State, OAuth2StateStore};

pub struct RedisStateStore {
    // Redis client
}

#[async_trait]
impl OAuth2StateStore for RedisStateStore {
    async fn store(&self, state: OAuth2State) -> OAuth2Result<()> {
        // SET with an expiry matching `state.expires_at`.
        todo!()
    }

    async fn retrieve(&self, state: &str) -> OAuth2Result<OAuth2State> {
        // GETDEL, so a second callback with the same state finds nothing.
        // Return OAuth2Error::StateNotFound when it is missing or expired.
        todo!()
    }

    async fn cleanup_expired(&self) -> OAuth2Result<usize> {
        // Redis expiry removes old states.
        Ok(0)
    }
}
```

## Checks

```bash
cargo test -p ras-identity-oauth2 --locked
cargo clippy -p ras-identity-oauth2 --all-targets --all-features --locked -- -D warnings
```
