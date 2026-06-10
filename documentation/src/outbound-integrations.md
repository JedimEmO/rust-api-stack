# Outbound Integrations

RAS services call other systems: third-party APIs (Google, GitHub, customer
systems) and other internal RAS services. `ras-integration-core` provides
the shared, fail-closed machinery so projects stop hand-rolling token
acquisition, caching, refresh, and bearer attachment.

## The pieces

- **`TokenSource`** — pluggable acquisition. Implementations:
  `OAuth2TokenSource` (`ras-integration-oauth2`), `RasInternalTokenSource`
  (`ras-integration-ras`), `StaticTokenSource` for API keys, and
  `testing::FakeTokenSource` for tests.
- **`IntegrationConfig`** — per-integration bounds: allowed scopes, allowed
  audiences, and allowed outbound base URLs. Anything outside the bounds
  fails before a token source is even consulted.
- **`TokenManager`** — caching and refresh. Cache keys include the token
  family, integration, subject (with principal mode), audience, canonical
  scopes, and config version, so external OAuth tokens and internal RAS
  tokens can never collide. Near-expiry leases refresh early; concurrent
  refreshes for the same key are deduplicated; errors are never cached.
- **`AuthorizedHttpClient`** — the capability-scoped client handlers should
  receive: bound to one integration, one subject, and a fixed scope set.
  Handlers cannot request arbitrary integrations, scopes, audiences, or
  subjects (the confused-deputy guard). The outbound URL is validated
  against the host allowlist *before* a token is minted, and requests are
  never automatically replayed after auth failures.
- **`GrantStore`** — persistence for refresh-token grants. A refresh token
  is a stored *grant*: the application provides it (consent flow, admin
  seeding, migration); RAS uses it. The store is a security boundary —
  implement it over your database/secret manager; the in-memory store
  serves tests and dev.
- **`SecretString`** — every token and grant secret is redacted in `Debug`
  and has no serde implementations.

## External OAuth2 providers

```rust,ignore
let source = Arc::new(OAuth2TokenSource::new(
    OAuth2ProviderConfig::new("https://accounts.google.com/o/oauth2/token", client_id)?
        .with_client_secret(client_secret),
    transport.clone(),   // any HttpTransport — fake the provider in tests
    grant_store.clone(),
)?);

let google = AuthorizedHttpClient::for_user(
    transport, manager, "google-calendar", user.user_id, ["calendar.readonly"],
);
let events = google.get(calendar_url).await?;
```

- **User subjects** use the refresh-token flow. Requested scopes are
  subset-checked against the stored grant's consented scopes; broader
  requests return a typed `ConsentRequired` error (as does a missing or
  revoked grant), never a silent fallback. Rotated refresh tokens are
  persisted back to the `GrantStore` before the lease is returned, and a
  failed save surfaces as an error.
- **Service subjects** use client credentials, forwarding the requested
  `audience` for providers that support it.

`ConsentFlow` covers the consent side: PKCE (S256) authorization URLs with
opaque, single-use, expiring `state` bound to the initiating user,
integration, redirect URI, scopes, and verifier; callback validation; and
the code exchange that stores the grant.

## Internal RAS services

Use `RasInternalTokenSource` (see
[Service-To-Service Auth](service-to-service-auth.md)). The same manager,
bounds, cache, and client machinery applies — only the source differs, and
the token family in the cache key keeps the two worlds apart.

## Testing

Everything speaks `HttpTransport`, so provider and downstream fakes run
in-process: script token-endpoint responses, assert the exact form
parameters sent, and exercise your real service code path instead of
mocking it away. `ras-integration-core::testing` ships a scriptable
`FakeTokenSource` with call counting for cache/dedup assertions.
