//! Deployment-agnostic RAS service topology (issue #15).
//!
//! Declare the *logical* service graph — services with audiences and
//! generated permission manifests, gateway profiles with routes, and
//! allowed service-to-service call edges — then validate it
//! deterministically and emit the artifacts the rest of the stack consumes:
//!
//! - **Authorization policy** ([`Topology::authz_policy_json`]):
//!   schema-compatible with `ras-authorization-core`'s `ServiceGraphPolicy`,
//!   so the authority refuses to mint service tokens outside the declared
//!   graph.
//! - **Gateway profiles** ([`Topology::gateway_profile_toml`]):
//!   schema-compatible with `ras-authorization-gateway`'s `GatewayProfile`;
//!   deployment-specific upstream bindings stay external.
//! - **Diagrams** ([`Topology::mermaid`]).
//!
//! Validation runs in [`TopologyBuilder::build`] and covers: unique service
//! ids and audiences, unique gateway ids, per-gateway route-prefix
//! uniqueness (cross-gateway conflicts are allowed), routes and call edges
//! referencing declared services, public-gateway exposure of private
//! services failing unless explicitly allowed, and edge permissions checked
//! against the *target* service's imported manifest (with an explicit
//! `call_with_custom_permissions` escape hatch).
//!
//! Artifacts are deterministic — identical topologies produce byte-identical
//! output with stable content-derived ids — so they diff cleanly in CI and
//! are auditable.
//!
//! The topology never owns deployment concerns: schedulers, DNS, ingress,
//! meshes, and upstream URLs are provided by the deployment when artifacts
//! are loaded.
//!
//! The `ras-topology-macro` crate provides the `ras_topology!` macro that
//! generates builder code from a declarative graph description, with typed
//! references to manifest functions and generated permission constants.

mod artifacts;
mod error;
mod model;

pub use artifacts::ARTIFACT_SCHEMA_VERSION;
pub use error::TopologyError;
pub use model::{
    CallEdge, Exposure, GatewayNode, RouteDecl, ServiceNode, Topology, TopologyBuilder,
};
