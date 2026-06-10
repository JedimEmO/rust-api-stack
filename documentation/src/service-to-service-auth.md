# Service-To-Service Auth

External identity providers answer *"who is this human?"*. RAS answers
*"what may this user or service do here?"*. The authorization crates add a
RAS-native control plane so internal services get identities, grants, and
short-lived tokens without registering application clients in Auth0/Entra —
and without every project hand-rolling token plumbing.

## One token model

Everything builds on `ras-authorization-token`, which defines a single
claims shape (`RasClaims`) for all RAS token families, distinguished by the
`typ` claim:

| `typ` | Token | Audience shape |
|---|---|---|
| `ras_web_session` | Browser web session | Multi-audience: permissions grouped per audience in `audience_permissions` |
| `ras_internal_access` | Internal service-to-service token | Single `aud` + flat permission list |
| `ras_gateway_access` | Gateway-derived backend token | Single `aud` + flat permission list |

Tokens are signed JWTs (ES256 recommended, EdDSA supported, HS256 only for
single-process shared-secret setups) with `kid`-based key rotation and JWKS
publication. `TokenValidator` pins the issuer, audience policy, expected
token types, and an algorithm allowlist (asymmetric-only by default), and
guards against key-type confusion by cross-checking the resolved key's
algorithm against the header.

Because validation is offline (JWKS), revocation latency is bounded by
token TTL: internal tokens default to 5 minutes, gateway tokens to 2.
Emergency revocation removes a retired key from the ring, immediately
killing everything it signed.

## The control plane

`ras-authorization-core` owns application authorization:

- **Registry**: services are registered with a unique audience.
- **Grants are audience-scoped**: a grant says "principal X may use
  permission P *at audience A*". The same permission string on two services
  never satisfies each other. Roles bundle audience-scoped permissions and
  bind to principals (users, services, service accounts, applications).
- **Manifests are the vocabulary**: import each service's generated
  permission manifest; grants of unknown permissions are rejected unless
  made through the explicit `grant_custom` path.
- **Identity is pluggable**: services prove themselves through a
  `ServiceIdentityVerifier`. The shipped `StaticSecretVerifier` is for
  development and simple deployments; production should adapt workload
  identity (Kubernetes service-account JWTs, SPIFFE/SPIRE, mTLS) behind the
  same trait.
- **Issuance fails closed** at every step: identity, registration,
  audience existence, audience-scoped grants, and — when a topology policy
  is loaded — declared service-graph edges with permission ceilings.
- **Audit**: registrations, grants, issuance outcomes, and key changes emit
  append-only events that never contain secrets or token values.

## Deployment presets

Start embedded; scale out only when you need to.

1. **Embedded (default)** — one axum process hosts the authority routes and
   the application:

   ```rust,ignore
   let issuer = Arc::new(TokenIssuer::builder(issuer_url, key, store, verifier).build());
   let app = my_service_router.merge(authority_router(issuer));
   // POST /auth/token and GET /auth/jwks.json live next to /api/*.
   ```

2. **Central authority** — several services validate tokens from one shared
   authority through its JWKS; callers use `HttpAuthority` instead of
   `EmbeddedAuthority`. Nothing else changes.

3. **Auth gateway** — browser frontends fanning out to multiple backends
   add the optional gateway (see [The Auth Gateway](auth-gateway.md)).

## Calling another service

The calling side uses the outbound token framework (see
[Outbound Integrations](outbound-integrations.md)). For internal calls,
`RasInternalTokenSource` requests tokens from the authority — it holds no
keys and never mints locally:

```rust,ignore
let source = Arc::new(RasInternalTokenSource::new(
    Arc::new(EmbeddedAuthority::new(issuer.clone())), // or HttpAuthority
    ServiceIdentityProof { service_id: "billing".into(), proof: secret_proof },
));
let manager = Arc::new(TokenManager::builder()
    .register(
        IntegrationConfig::new("invoice-service", ["invoice:read"], [invoice_url])?
            .with_allowed_audiences(["invoice-service"]),
        source,
    )?
    .build());

// In a handler: lease a token (cached, deduplicated) and call the
// generated client.
let lease = manager.get_token(TokenRequest { /* service subject, audience */ }).await?;
client.set_bearer_token(Some(lease.access_token.expose_secret()));
```

## Accepting RAS tokens

The receiving side validates with `RasTokenAuthProvider`, a standard
`ras-auth-core` `AuthProvider`, so generated services enforce their
existing `WITH_PERMISSIONS` requirements unchanged:

```rust,ignore
let provider = RasTokenAuthProvider::new(TokenValidator::new(
    authority_jwks,
    ValidationOptions::new(issuer_url,
        AudiencePolicy::Exact("invoice-service".into()),
        vec![TokenType::InternalService]),
));
let app = InvoiceServiceBuilder::new(service).auth_provider(provider).build();
```

A service that also sits behind the gateway composes two providers (one for
`ras_internal_access`, one for `ras_gateway_access`) — see
`examples/authorization-demo` for the complete wiring.

## Relationship to existing identity crates

`ras-identity-*` (identity providers, `SessionService`) continues to
authenticate humans and issue legacy HMAC sessions. The authorization
layer's web-session model (`RasClaims::web_session` with audience-grouped
permissions) is what the gateway consumes; migrating `SessionService` onto
`ras-authorization-token` signing is the planned follow-up and will be a
breaking change for session-token consumers.
