# ras-authorization-token

Shared RAS token primitives: one claims model for web sessions, internal
service-to-service tokens, and gateway-derived backend tokens, with signing,
key rotation, JWKS, and a strict validation pipeline.

- Token families carried in `typ`: `ras_web_session` (multi-audience,
  permissions grouped per audience), `ras_internal_access` and
  `ras_gateway_access` (single-audience).
- Algorithms: ES256 (recommended default), EdDSA, and HS256 for embedded
  shared-secret deployments. Validators allow only asymmetric algorithms
  unless HS256 is explicitly opted in. `alg: none` and unknown algorithms are
  always rejected.
- `KeyRing` keeps retired verification keys through rotations so outstanding
  tokens stay valid, supports emergency key removal, and publishes JWKS
  (asymmetric keys only — HMAC secrets never leave the process).
- `TokenValidator` checks algorithm allowlist, `kid` resolution, key/header
  algorithm agreement (key-type-confusion guard), signature, token type,
  issuer, audience policy, expiry/not-before with clock skew, and per-family
  claim invariants.

This crate is deliberately HTTP-free; serving and fetching JWKS endpoints is
the job of the authorization control plane and gateway crates built on top.

See the crate documentation for usage examples.
