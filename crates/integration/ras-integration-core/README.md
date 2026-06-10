# ras-integration-core

Outbound token framework for RAS services (issue #12): pluggable
`TokenSource`s, a bounds-checked caching `TokenManager`, grant stores, and
capability-scoped `AuthorizedHttpClient`s built on `ras-transport-core`.

Design rules, all fail-closed:

- Handlers receive capability-scoped clients (one integration, one subject,
  fixed scopes) — never the raw token manager, so user-controlled input can
  never select an integration, scope, audience, or subject.
- Integration configs declare allowed scopes, audiences, and outbound base
  URLs; requests outside the bounds fail before any token source is
  consulted, and bearer tokens are only attached after exact-host validation.
- Cache keys include token family, integration, subject (with principal
  mode), audience, canonical scopes, and config version, so token families
  and principals cannot collide.
- Concurrent refreshes for the same key are deduplicated; near-expiry leases
  refresh early with configurable skew; errors are never cached.
- All secrets live in `SecretString` (redacted `Debug`, no serde).
- No automatic replay of requests after auth failures — retrying
  non-idempotent calls is always an explicit caller decision.

Token sources ship separately: `ras-integration-oauth2` (external
OAuth2/OIDC providers), `ras-integration-ras` (RAS-issued internal service
tokens), and `testing::FakeTokenSource`/`StaticTokenSource` here for tests
and legacy adapters.
