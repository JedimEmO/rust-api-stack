# ras-integration-oauth2

OAuth2/OIDC token source for the RAS outbound token framework
(`ras-integration-core`).

- Refresh-token flow for user subjects backed by stored grants, with scope
  subset-checking against the consented scopes and refresh-token rotation
  persisted to the `GrantStore` before a lease is returned.
- Client-credentials flow for service subjects, with optional `audience`
  forwarding.
- `invalid_grant` responses surface as typed `ConsentRequired` errors so
  applications can route users back through consent.
- `ConsentFlow`: PKCE (S256) authorization URLs with opaque, single-use,
  expiring `state` bound to the initiating user, integration, redirect URI,
  scopes, and verifier; callback validation; authorization-code exchange that
  stores the resulting grant.
- Provider endpoints must be https unless explicitly opted into insecure
  http for in-process test fakes.

All HTTP goes through `ras-transport-core`, so the provider can be faked
in-process for tests.
