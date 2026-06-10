# ras-integration-ras

`RasInternalTokenSource`: the bridge between the RAS outbound token
framework (`ras-integration-core`) and the RAS authorization control plane
(`ras-authorization-core`).

- Holds no signing keys and never mints locally — every lease comes from a
  successful authorization decision by the RAS authority.
- `EmbeddedAuthority` calls the issuer in-process (embedded preset);
  `HttpAuthority` posts to a central authority's `POST /auth/token`.
- v1 issues service-as-service tokens only; user-delegated and
  service-account requests fail closed before any authority call, and the
  request/cache model already distinguishes principal modes for later.
- Authority denials surface as typed `Denied` errors; authority/transport
  failures as `Provider` errors.
