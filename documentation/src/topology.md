# Topology

Once several RAS services, an authority, and a gateway exist, hand-written
route/audience/grant configuration drifts: gateway routes outlive renamed
APIs, service grants reference deleted permissions, and a public gateway
can quietly expose a private service. The topology crates apply the RAS
philosophy one level up: declare the *logical* service graph in Rust,
validate it deterministically, and generate the artifacts everything else
consumes.

## Declaring a topology

```rust,ignore
ras_topology!({
    topology_name: InternalTools,

    services: [
        invoice: {
            audience: "invoice-service",
            manifest: invoice_api::generate_invoiceservice_permission_manifest,
            exposure: private,
        },
        billing: {
            audience: "billing-service",
            manifest: billing_api::generate_billingservice_permission_manifest,
            exposure: private,
        },
    ],

    gateways: [
        public_web: {
            exposure: public,
            routes: [
                "/api/invoice" => invoice { expose_private },
                "/api/billing" => billing { expose_private },
            ],
        },
    ],

    calls: [
        billing -> invoice {
            permissions: [invoice_api::invoiceservice_permissions::INVOICE_READ],
        },
    ],
});
```

This generates `internal_tools_topology() -> Result<Topology,
TopologyError>`. The topology crate sits *downstream* of the service API
crates, so the references are typed: a renamed manifest function or removed
permission constant fails the topology build immediately.

## Validation levels

- **Compile time** (the macro): duplicate service/gateway ids, routes and
  call edges referencing undeclared services, and — via the typed paths —
  existence of manifest functions and permission constants.
- **Build/test time** (`build()`): audience uniqueness, per-gateway route
  conflicts (the same prefix on *different* gateways is fine), exposure
  rules (a public gateway exposing a private service fails unless the route
  is explicitly `expose_private`), and edge permissions checked against the
  *target* service's manifest. Raw permission strings require the explicit
  `custom_permissions` escape hatch. Run the generated function in a test
  and the graph is checked on every CI build.
- **Startup time** (the consumers): deployment-provided upstream bindings
  are validated when a gateway profile is loaded.

## Generated artifacts

All artifacts are byte-deterministic with stable content-derived ids, so
they diff cleanly in PRs and serve as audit input:

- `topology.authz_policy_json()` — the allowed service-graph edges with
  permission ceilings. Loads directly into the authority
  (`issuer.load_policy(...)`), which then refuses to mint internal tokens
  for undeclared edges or permissions beyond an edge's ceiling, *in
  addition to* the grant checks.
- `topology.gateway_profile_toml("public_web")` — route → audience config
  per gateway profile. Loads into
  `GatewayConfig::from_profile_toml(...)` together with the deployment's
  audience → upstream URL bindings.
- `topology.mermaid()` — a flowchart of gateways, services, routes, and
  call edges for docs and reviews.

## What topology does not own

Deployment substrates: Kubernetes vs Compose vs systemd, ingress
controllers, DNS, meshes, certificates, and rollout lifecycle all stay
external. The topology dictates *auth* topology; the deployment binds
logical names to concrete upstreams when it loads the artifacts.
