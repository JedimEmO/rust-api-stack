# ras-topology-core

Deployment-agnostic RAS service topology (issue #15): declare the logical
service graph — services with audiences and generated permission manifests,
gateway profiles with routes, allowed service-to-service edges — validate it
deterministically, and emit the artifacts the rest of the stack consumes:

- authorization policy JSON (loads into `ras-authorization-core`'s
  `ServiceGraphPolicy`, constraining internal token issuance to declared
  edges),
- gateway profile TOML per gateway (loads into
  `ras-authorization-gateway`'s `GatewayConfig::from_profile_toml`, with
  upstream bindings supplied by the deployment),
- Mermaid diagrams.

Validation covers unique service ids/audiences, gateway route conflicts
(per-profile; cross-profile conflicts allowed), undeclared references,
public-gateway exposure of private services (explicit `expose_private`
required), and edge permissions checked against the target service's
manifest (explicit `call_with_custom_permissions` escape hatch).

Artifacts are byte-deterministic with stable content-derived ids, so they
diff cleanly in CI and serve as audit input. Deployment concerns (DNS,
schedulers, ingress, upstream URLs) deliberately stay out of the model.

Use `ras-topology-macro`'s `ras_topology!` for the declarative form with
typed references to manifest functions and permission constants.
