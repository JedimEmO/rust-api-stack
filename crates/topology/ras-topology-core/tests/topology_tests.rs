//! Topology validation, deterministic artifacts, and consumption by the
//! authorization control plane and the gateway.

use std::collections::BTreeMap;

use ras_permission_manifest::{
    AuthRequirementInfo, OperationKind, OperationPermissions, PermissionManifest,
    ServicePermissions, TransportKind, WireTarget,
};
use ras_topology_core::{Exposure, RouteDecl, Topology, TopologyError};

fn manifest(service: &str, permissions: &[&str]) -> PermissionManifest {
    let operations = permissions
        .iter()
        .map(|permission| OperationPermissions {
            operation_id: format!("op_{permission}"),
            operation_name: format!("op_{permission}"),
            kind: OperationKind::RestEndpoint,
            wire: WireTarget::Rest {
                method: "GET".to_string(),
                path: format!("/{permission}"),
            },
            auth: AuthRequirementInfo::from_permission_groups([[*permission]]),
            version: None,
            canonical_operation_id: None,
        })
        .collect();
    PermissionManifest::from_services([ServicePermissions {
        service_name: service.to_string(),
        transport: TransportKind::Rest,
        operations,
    }])
}

fn base_builder() -> ras_topology_core::TopologyBuilder {
    Topology::builder("InternalTools")
        .service(
            "invoice",
            "invoice-service",
            Exposure::Private,
            manifest("InvoiceService", &["invoice:read", "invoice:write"]),
        )
        .service(
            "billing",
            "billing-service",
            Exposure::Private,
            manifest("BillingService", &["billing:read"]),
        )
}

#[test]
fn valid_topology_builds() {
    let topology = base_builder()
        .gateway(
            "public_web",
            Exposure::Public,
            vec![
                RouteDecl::new("/invoices", "invoice").expose_private(),
                RouteDecl::new("/billing", "billing").expose_private(),
            ],
        )
        .call("billing", "invoice", ["invoice:read"])
        .build()
        .unwrap();
    assert_eq!(topology.name(), "InternalTools");
    assert_eq!(topology.services().len(), 2);
}

#[test]
fn duplicate_ids_and_audiences_are_rejected() {
    let err = base_builder()
        .service(
            "invoice",
            "other-audience",
            Exposure::Private,
            manifest("X", &[]),
        )
        .build()
        .unwrap_err();
    assert_eq!(
        err,
        TopologyError::DuplicateServiceId("invoice".to_string())
    );

    let err = base_builder()
        .service(
            "invoice2",
            "invoice-service",
            Exposure::Private,
            manifest("X", &[]),
        )
        .build()
        .unwrap_err();
    assert!(matches!(err, TopologyError::DuplicateAudience { .. }));
}

#[test]
fn gateway_validation_catches_conflicts_unknown_targets_and_exposure() {
    // Duplicate prefix within one gateway.
    let err = base_builder()
        .gateway(
            "gw",
            Exposure::Private,
            vec![
                RouteDecl::new("/x", "invoice"),
                RouteDecl::new("/x", "billing"),
            ],
        )
        .build()
        .unwrap_err();
    assert!(matches!(err, TopologyError::DuplicateRoutePrefix { .. }));

    // The same prefix on *different* gateways is fine.
    base_builder()
        .gateway(
            "gw1",
            Exposure::Private,
            vec![RouteDecl::new("/x", "invoice")],
        )
        .gateway(
            "gw2",
            Exposure::Private,
            vec![RouteDecl::new("/x", "billing")],
        )
        .build()
        .unwrap();

    // Unknown route target.
    let err = base_builder()
        .gateway("gw", Exposure::Private, vec![RouteDecl::new("/x", "ghost")])
        .build()
        .unwrap_err();
    assert!(matches!(err, TopologyError::UnknownRouteTarget { .. }));

    // Public gateway exposing a private service fails by default...
    let err = base_builder()
        .gateway(
            "public_web",
            Exposure::Public,
            vec![RouteDecl::new("/invoices", "invoice")],
        )
        .build()
        .unwrap_err();
    assert!(matches!(
        err,
        TopologyError::PublicGatewayExposesPrivateService { .. }
    ));

    // ...unless explicitly allowed.
    base_builder()
        .gateway(
            "public_web",
            Exposure::Public,
            vec![RouteDecl::new("/invoices", "invoice").expose_private()],
        )
        .build()
        .unwrap();
}

#[test]
fn call_edges_are_validated_against_target_manifests() {
    // Unknown endpoint.
    let err = base_builder()
        .call("ghost", "invoice", ["invoice:read"])
        .build()
        .unwrap_err();
    assert_eq!(err, TopologyError::UnknownCallService("ghost".to_string()));

    // Permission not in the target manifest (billing:read belongs to the
    // *caller*, not the target — audience scoping in action).
    let err = base_builder()
        .call("billing", "invoice", ["billing:read"])
        .build()
        .unwrap_err();
    assert!(matches!(
        err,
        TopologyError::PermissionNotInTargetManifest { permission, .. } if permission == "billing:read"
    ));

    // The explicit custom path skips manifest validation.
    base_builder()
        .call_with_custom_permissions("billing", "invoice", ["legacy:perm"])
        .build()
        .unwrap();
}

#[test]
fn artifacts_are_deterministic() {
    let build = || {
        base_builder()
            .gateway(
                "public_web",
                Exposure::Public,
                vec![RouteDecl::new("/invoices", "invoice").expose_private()],
            )
            .call("billing", "invoice", ["invoice:read", "invoice:write"])
            .build()
            .unwrap()
    };
    let first = build();
    let second = build();
    assert_eq!(
        first.authz_policy_json().unwrap(),
        second.authz_policy_json().unwrap()
    );
    assert_eq!(
        first.gateway_profile_toml("public_web").unwrap(),
        second.gateway_profile_toml("public_web").unwrap()
    );
    assert_eq!(first.mermaid(), second.mermaid());

    // Changing the topology changes the artifact ids.
    let other = base_builder()
        .gateway(
            "public_web",
            Exposure::Public,
            vec![RouteDecl::new("/invoices", "invoice").expose_private()],
        )
        .call("billing", "invoice", ["invoice:read"])
        .build()
        .unwrap();
    assert_ne!(
        first.authz_policy_json().unwrap(),
        other.authz_policy_json().unwrap()
    );
}

#[test]
fn authz_policy_artifact_loads_into_the_control_plane() {
    let topology = base_builder()
        .call("billing", "invoice", ["invoice:read"])
        .build()
        .unwrap();
    let json = topology.authz_policy_json().unwrap();

    // The artifact is schema-compatible with the authority's policy type.
    let policy: ras_authorization_core::ServiceGraphPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(policy.topology_name, "InternalTools");
    assert!(policy.policy_id.starts_with("InternalTools@"));
    let edge = policy.edge("billing", "invoice-service").unwrap();
    assert!(edge.permissions.contains("invoice:read"));
    assert!(policy.edge("invoice", "billing-service").is_none());
}

#[test]
fn gateway_profile_artifact_loads_into_the_gateway() {
    let topology = base_builder()
        .gateway(
            "public_web",
            Exposure::Public,
            vec![
                RouteDecl::new("/invoices", "invoice").expose_private(),
                RouteDecl::new("/billing", "billing")
                    .expose_private()
                    .authenticated_only(),
            ],
        )
        .build()
        .unwrap();
    let toml = topology.gateway_profile_toml("public_web").unwrap();

    // Missing upstream binding fails startup validation.
    let mut upstreams = BTreeMap::new();
    upstreams.insert(
        "invoice-service".to_string(),
        "http://invoice:3000".to_string(),
    );
    assert!(
        ras_authorization_gateway::GatewayConfig::from_profile_toml(
            "https://auth.internal",
            "https://gateway.internal",
            &toml,
            &upstreams,
        )
        .is_err()
    );

    // Complete bindings load.
    upstreams.insert(
        "billing-service".to_string(),
        "http://billing:3000".to_string(),
    );
    let config = ras_authorization_gateway::GatewayConfig::from_profile_toml(
        "https://auth.internal",
        "https://gateway.internal",
        &toml,
        &upstreams,
    )
    .unwrap();
    assert_eq!(config.routes.len(), 2);
    let billing = config
        .routes
        .iter()
        .find(|route| route.audience == "billing-service")
        .unwrap();
    assert!(billing.authenticated_only);

    // Unknown gateway id fails.
    assert!(matches!(
        topology.gateway_profile_toml("nope").unwrap_err(),
        TopologyError::UnknownGateway(_)
    ));
}

#[test]
fn mermaid_diagram_contains_nodes_routes_and_edges() {
    let topology = base_builder()
        .gateway(
            "public_web",
            Exposure::Public,
            vec![RouteDecl::new("/invoices", "invoice").expose_private()],
        )
        .call("billing", "invoice", ["invoice:read"])
        .build()
        .unwrap();
    let mermaid = topology.mermaid();
    assert!(mermaid.starts_with("flowchart LR"));
    assert!(mermaid.contains("public_web"));
    assert!(mermaid.contains("invoice-service"));
    assert!(mermaid.contains("-->|/invoices| invoice"));
    assert!(mermaid.contains("billing -.->|invoice:read| invoice"));
}
