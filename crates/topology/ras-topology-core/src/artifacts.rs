//! Deterministic artifact emission: authorization policy, gateway profiles,
//! and diagrams.
//!
//! Every artifact carries the schema version, the topology name, and a
//! deterministic content-derived id, so CI diffs are meaningful and
//! authorization decisions can be traced to the exact topology revision
//! that produced them.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::TopologyError;
use crate::model::{Exposure, Topology};

/// Artifact schema version.
pub const ARTIFACT_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct PolicyArtifact<'a> {
    schema_version: u32,
    topology_name: &'a str,
    policy_id: String,
    edges: Vec<PolicyEdge>,
}

#[derive(Serialize)]
struct PolicyEdge {
    caller_service_id: String,
    target_audience: String,
    permissions: Vec<String>,
}

#[derive(Serialize)]
struct ProfileArtifact<'a> {
    schema_version: u32,
    topology: &'a str,
    profile: &'a str,
    profile_id: String,
    routes: std::collections::BTreeMap<String, ProfileRouteArtifact>,
}

#[derive(Serialize)]
struct ProfileRouteArtifact {
    audience: String,
    authenticated_only: bool,
}

/// Short deterministic content hash for stable artifact ids.
fn content_id(payload: &str) -> String {
    let digest = Sha256::digest(payload.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Topology {
    /// The authorization policy artifact: allowed service-graph edges with
    /// their permission ceilings, as pretty JSON.
    ///
    /// Schema-compatible with `ras-authorization-core`'s
    /// `ServiceGraphPolicy`, so the authority can load it directly and
    /// refuse token issuance outside the declared graph.
    pub fn authz_policy_json(&self) -> Result<String, TopologyError> {
        let mut edges: Vec<PolicyEdge> = self
            .calls()
            .iter()
            .map(|call| {
                let target = self
                    .service(&call.target_id)
                    .expect("validated at build time");
                PolicyEdge {
                    caller_service_id: call.caller_id.clone(),
                    target_audience: target.audience.clone(),
                    permissions: call.permissions.iter().cloned().collect(),
                }
            })
            .collect();
        edges.sort_by(|left, right| {
            (&left.caller_service_id, &left.target_audience)
                .cmp(&(&right.caller_service_id, &right.target_audience))
        });

        // The id is derived from the edge content itself, so identical
        // topologies produce identical artifacts byte-for-byte.
        let content = serde_json::to_string(&edges)
            .map_err(|err| TopologyError::Invalid(format!("policy serialization: {err}")))?;
        let artifact = PolicyArtifact {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            topology_name: self.name(),
            policy_id: format!("{}@{}", self.name(), content_id(&content)),
            edges,
        };
        serde_json::to_string_pretty(&artifact)
            .map_err(|err| TopologyError::Invalid(format!("policy serialization: {err}")))
    }

    /// The gateway profile artifact for one declared gateway, as TOML.
    ///
    /// Schema-compatible with `ras-authorization-gateway`'s
    /// `GatewayProfile`; deployment-specific upstream bindings are
    /// deliberately absent.
    pub fn gateway_profile_toml(&self, gateway_id: &str) -> Result<String, TopologyError> {
        let gateway = self
            .gateways()
            .iter()
            .find(|gateway| gateway.id == gateway_id)
            .ok_or_else(|| TopologyError::UnknownGateway(gateway_id.to_string()))?;

        let routes: std::collections::BTreeMap<String, ProfileRouteArtifact> = gateway
            .routes
            .iter()
            .map(|route| {
                let target = self
                    .service(&route.service_id)
                    .expect("validated at build time");
                (
                    route.prefix.clone(),
                    ProfileRouteArtifact {
                        audience: target.audience.clone(),
                        authenticated_only: route.authenticated_only,
                    },
                )
            })
            .collect();

        let content = serde_json::to_string(&routes)
            .map_err(|err| TopologyError::Invalid(format!("profile serialization: {err}")))?;
        let artifact = ProfileArtifact {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            topology: self.name(),
            profile: &gateway.id,
            profile_id: format!("{}/{}@{}", self.name(), gateway.id, content_id(&content)),
            routes,
        };
        toml::to_string_pretty(&artifact)
            .map_err(|err| TopologyError::Invalid(format!("profile serialization: {err}")))
    }

    /// A Mermaid flowchart of the topology: gateways, services, routes, and
    /// call edges with their permissions. Contains no secrets by
    /// construction (the model holds none).
    pub fn mermaid(&self) -> String {
        let mut out = String::from("flowchart LR\n");
        for gateway in self.gateways() {
            let shape = match gateway.exposure {
                Exposure::Public => {
                    format!("{}([\"{} (public gateway)\"])", gateway.id, gateway.id)
                }
                Exposure::Private => {
                    format!("{}([\"{} (private gateway)\"])", gateway.id, gateway.id)
                }
            };
            out.push_str(&format!("    {shape}\n"));
        }
        for service in self.services() {
            out.push_str(&format!(
                "    {}[\"{} ({})\"]\n",
                service.id, service.id, service.audience
            ));
        }
        for gateway in self.gateways() {
            for route in &gateway.routes {
                out.push_str(&format!(
                    "    {} -->|{}| {}\n",
                    gateway.id, route.prefix, route.service_id
                ));
            }
        }
        for call in self.calls() {
            let permissions: Vec<&str> = call.permissions.iter().map(String::as_str).collect();
            out.push_str(&format!(
                "    {} -.->|{}| {}\n",
                call.caller_id,
                permissions.join(", "),
                call.target_id
            ));
        }
        out
    }
}
