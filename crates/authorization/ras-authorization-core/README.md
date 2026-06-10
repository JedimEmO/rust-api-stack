# ras-authorization-core

RAS-native authorization control plane (issue #13, embedded mode): external
identity providers authenticate humans, RAS owns application authorization.

- **Audience-scoped grants**: "principal X may use permission P at audience
  A" — identical permission strings on different services never satisfy
  each other. Roles bundle audience-scoped permissions; principals are
  users, services, service accounts, or applications.
- **Manifest-driven vocabulary**: import generated permission manifests per
  audience; grants of unknown permissions are rejected unless made through
  the explicit `grant_custom` path.
- **Pluggable service identity**: `ServiceIdentityVerifier` trait with a
  constant-time static-secret verifier for dev/simple deployments;
  production should adapt workload identity (Kubernetes SA JWTs,
  SPIFFE/SPIRE, mTLS) behind the same trait.
- **Fail-closed token issuance**: identity → registration → audience
  existence → audience-scoped grants → (optional) topology
  `ServiceGraphPolicy` edges with permission ceilings, minting short-lived
  single-audience `ras_internal_access` JWTs stamped with `authz_version`.
- **JWKS + rotation**: downstream services validate offline; emergency
  removal of retired keys immediately invalidates their tokens.
- **Append-only audit**: registrations, grants, issuance outcomes, and key
  changes — never containing secrets or token values.
- **Embedded authority routes**: `authority_router` mounts
  `POST /auth/token` and `GET /auth/jwks.json` into any axum app (the
  default deployment preset); central-authority deployments serve the same
  router standalone.
- **Downstream validation**: `RasTokenAuthProvider` implements
  `ras-auth-core`'s `AuthProvider`, so existing generated services accept
  RAS internal and gateway tokens with their `WITH_PERMISSIONS`
  enforcement unchanged.

Storage is trait-based (`AuthorizationStore`); the in-memory implementation
serves embedded mode, tests, and examples. See the
`examples/authorization-demo` crate and the Service-To-Service Auth book
chapter for full wiring.
