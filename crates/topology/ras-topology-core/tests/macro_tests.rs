//! `ras_topology!` macro tests: the generated function builds a validated
//! topology with typed references to manifest functions and permission
//! constants.

use ras_topology_macro::ras_topology;

/// Stand-in for a generated service API crate.
mod invoice_api {
    use ras_permission_manifest::{
        AuthRequirementInfo, OperationKind, OperationPermissions, PermissionManifest,
        PermissionRef, ServicePermissions, TransportKind, WireTarget,
    };

    pub mod invoiceservice_permissions {
        use super::PermissionRef;
        pub const INVOICE_READ: PermissionRef = PermissionRef::new("invoice:read");
        pub const INVOICE_WRITE: PermissionRef = PermissionRef::new("invoice:write");
    }

    pub fn generate_invoiceservice_permission_manifest() -> PermissionManifest {
        PermissionManifest::from_services([ServicePermissions {
            service_name: "InvoiceService".to_string(),
            transport: TransportKind::Rest,
            operations: vec![OperationPermissions {
                operation_id: "list_invoices".to_string(),
                operation_name: "list_invoices".to_string(),
                kind: OperationKind::RestEndpoint,
                wire: WireTarget::Rest {
                    method: "GET".to_string(),
                    path: "/invoices".to_string(),
                },
                auth: AuthRequirementInfo::from_permission_groups([[
                    invoiceservice_permissions::INVOICE_READ.as_str(),
                    invoiceservice_permissions::INVOICE_WRITE.as_str(),
                ]]),
                version: None,
                canonical_operation_id: None,
            }],
        }])
    }
}

mod billing_api {
    use ras_permission_manifest::{PermissionManifest, ServicePermissions, TransportKind};

    pub fn generate_billingservice_permission_manifest() -> PermissionManifest {
        PermissionManifest::from_services([ServicePermissions {
            service_name: "BillingService".to_string(),
            transport: TransportKind::Rest,
            operations: vec![],
        }])
    }
}

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
                "/invoices" => invoice { expose_private },
                "/billing" => billing { expose_private, authenticated_only },
            ],
        },
        internal_admin: {
            exposure: private,
            routes: [
                "/invoices" => invoice,
            ],
        },
    ],

    calls: [
        billing -> invoice {
            permissions: [
                invoice_api::invoiceservice_permissions::INVOICE_READ,
                invoice_api::invoiceservice_permissions::INVOICE_WRITE,
            ],
        },
    ],
});

#[test]
fn generated_function_builds_a_validated_topology() {
    let topology = internal_tools_topology().unwrap();
    assert_eq!(topology.name(), "InternalTools");
    assert_eq!(topology.services().len(), 2);
    assert_eq!(topology.gateways().len(), 2);
    assert_eq!(topology.calls().len(), 1);

    // Typed permission constants flowed into the edge.
    let policy_json = topology.authz_policy_json().unwrap();
    let policy: ras_authorization_core::ServiceGraphPolicy =
        serde_json::from_str(&policy_json).unwrap();
    let edge = policy.edge("billing", "invoice-service").unwrap();
    assert!(edge.permissions.contains("invoice:read"));
    assert!(edge.permissions.contains("invoice:write"));

    // Both gateway profiles emit independently.
    assert!(topology.gateway_profile_toml("public_web").is_ok());
    assert!(topology.gateway_profile_toml("internal_admin").is_ok());

    // Route flags survived: billing route is authenticated-only.
    let toml = topology.gateway_profile_toml("public_web").unwrap();
    assert!(toml.contains("authenticated_only = true"));
}
