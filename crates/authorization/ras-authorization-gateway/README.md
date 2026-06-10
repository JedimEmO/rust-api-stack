# ras-authorization-gateway

Optional RAS auth gateway (issue #14): a token-narrowing reverse proxy for
browser frontends that fan out to multiple backend services. Deploy it
*behind* your existing ingress — it is an application-layer token exchanger,
not a general-purpose ingress controller.

- Validates `ras_web_session` tokens locally via JWKS — no authority call on
  the hot path.
- Deterministic longest-prefix, segment-aligned route matching; duplicate
  prefixes fail validation; unmatched routes fail closed.
- Mints short-lived single-audience `ras_gateway_access` tokens containing
  only the target audience's session permissions; never invents or widens;
  never forwards the original session token.
- Derived tokens never outlive their session; the derived-token cache
  (keyed by session/subject/audience/authz-version) is an optimization only.
- Strips inbound `Authorization`, `Cookie`, `Host`, and hop-by-hop headers;
  streams request/response bodies without buffering.
- Sessions lacking the target audience's permissions fail closed unless the
  route is explicitly `authenticated_only`.
- **v1 limitation:** connection upgrades (WebSocket) fail closed with 501;
  bidirectional RAS services must be reached directly for now.
- Consumes generated topology gateway profiles
  (`GatewayConfig::from_profile_toml`) with startup validation of
  deployment-provided upstream bindings; hand-written `RouteRule`s remain
  supported.

Backends validate derived tokens with `backend_validation_options` + the
gateway JWKS, or `ras-authorization-core`'s `RasTokenAuthProvider`.
