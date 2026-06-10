//! Control-plane tests: store semantics, fail-closed issuance, topology
//! policy, key rotation, audit, and the embedded authority router.

use std::collections::BTreeSet;
use std::sync::Arc;

use ras_authorization_core::{
    AudiencePermission, AuditEventKind, AuthorizationStore, AuthzError, InMemoryAuditSink,
    InMemoryAuthorizationStore, InternalTokenRequest, Principal, RoleDefinition, ServiceEdge,
    ServiceGraphPolicy, ServiceIdentityProof, ServiceRegistration, StaticSecretVerifier,
    TokenIssuer, authority_router,
};
use ras_authorization_token::{
    AudiencePolicy, JwkSet, SigningKey, TokenType, TokenValidator, ValidationOptions,
};
use ras_permission_manifest::{
    AuthRequirementInfo, OperationKind, OperationPermissions, PermissionManifest,
    ServicePermissions, TransportKind, WireTarget,
};

const ISSUER: &str = "https://auth.internal";
const BILLING_SECRET: &str = "billing-service-static-secret-32b!!";

/// Build a manifest whose operations require the given permissions.
fn manifest(service: &str, permissions: &[&str]) -> PermissionManifest {
    let operations = permissions
        .iter()
        .map(|permission| OperationPermissions {
            operation_id: format!("op_{permission}"),
            operation_name: format!("op_{permission}"),
            kind: OperationKind::JsonRpcMethod,
            wire: WireTarget::JsonRpc {
                method: permission.to_string(),
            },
            auth: AuthRequirementInfo::from_permission_groups([[*permission]]),
            version: None,
            canonical_operation_id: None,
        })
        .collect();
    PermissionManifest::from_services([ServicePermissions {
        service_name: service.to_string(),
        transport: TransportKind::JsonRpc,
        operations,
    }])
}

struct Authority {
    store: Arc<InMemoryAuthorizationStore>,
    verifier: Arc<StaticSecretVerifier>,
    audit: Arc<InMemoryAuditSink>,
    issuer: Arc<TokenIssuer>,
}

/// Standard fixture: billing-service and invoice-service registered,
/// invoice manifest imported, billing granted invoice:read at
/// invoice-service.
async fn authority() -> Authority {
    let store = Arc::new(InMemoryAuthorizationStore::new());
    let verifier = Arc::new(StaticSecretVerifier::new());
    let audit = Arc::new(InMemoryAuditSink::new());

    for (id, audience) in [
        ("billing-service", "billing-service"),
        ("invoice-service", "invoice-service"),
    ] {
        store
            .register_service(ServiceRegistration {
                service_id: id.to_string(),
                display_name: id.to_string(),
                audience: audience.to_string(),
                enabled: true,
            })
            .await
            .unwrap();
    }
    verifier
        .register("billing-service", BILLING_SECRET.as_bytes())
        .await
        .unwrap();

    store
        .import_manifest(
            "invoice-service",
            &manifest("InvoiceService", &["invoice:read", "invoice:write"]),
        )
        .await
        .unwrap();
    store
        .grant(
            Principal::Service {
                service_id: "billing-service".to_string(),
            },
            AudiencePermission::new("invoice-service", "invoice:read"),
        )
        .await
        .unwrap();

    let issuer = Arc::new(
        TokenIssuer::builder(
            ISSUER,
            SigningKey::generate_es256("k1"),
            store.clone(),
            verifier.clone(),
        )
        .audit(audit.clone())
        .build(),
    );

    Authority {
        store,
        verifier,
        audit,
        issuer,
    }
}

fn billing_proof(secret: &str) -> ServiceIdentityProof {
    ServiceIdentityProof {
        service_id: "billing-service".to_string(),
        proof: serde_json::json!({ "client_secret": secret }),
    }
}

fn token_request(permissions: &[&str]) -> InternalTokenRequest {
    InternalTokenRequest {
        proof: billing_proof(BILLING_SECRET),
        audience: "invoice-service".to_string(),
        permissions: permissions.iter().map(|p| p.to_string()).collect(),
    }
}

fn invoice_validator(jwks: JwkSet) -> TokenValidator<JwkSet> {
    TokenValidator::new(
        jwks,
        ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::InternalService],
        ),
    )
}

// --- Store semantics ---

#[tokio::test]
async fn grants_are_audience_scoped() {
    let authority = authority().await;
    // billing has invoice:read at invoice-service. The same permission
    // string at billing-service must not satisfy anything.
    let principal = Principal::Service {
        service_id: "billing-service".to_string(),
    };
    let resolved = authority
        .store
        .resolve_permissions(&principal)
        .await
        .unwrap();
    assert!(resolved["invoice-service"].contains("invoice:read"));
    assert!(!resolved.contains_key("billing-service"));
}

#[tokio::test]
async fn roles_and_direct_grants_merge_in_resolution() {
    let authority = authority().await;
    let principal = Principal::User {
        user_id: "alice".to_string(),
    };
    authority
        .store
        .define_role(RoleDefinition {
            role_id: "invoice-admin".to_string(),
            permissions: BTreeSet::from([
                AudiencePermission::new("invoice-service", "invoice:read"),
                AudiencePermission::new("invoice-service", "invoice:write"),
            ]),
        })
        .await
        .unwrap();
    authority
        .store
        .bind_role(principal.clone(), "invoice-admin")
        .await
        .unwrap();
    authority
        .store
        .grant(
            principal.clone(),
            AudiencePermission::new("invoice-service", "invoice:read"),
        )
        .await
        .unwrap();

    let resolved = authority
        .store
        .resolve_permissions(&principal)
        .await
        .unwrap();
    assert_eq!(resolved["invoice-service"].len(), 2);
}

#[tokio::test]
async fn unknown_permissions_are_rejected_unless_custom() {
    let authority = authority().await;
    let principal = Principal::User {
        user_id: "alice".to_string(),
    };

    let err = authority
        .store
        .grant(
            principal.clone(),
            AudiencePermission::new("invoice-service", "not-in-any-manifest"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::UnknownPermission { .. }));

    // Known permission at the wrong audience is also unknown.
    let err = authority
        .store
        .grant(
            principal.clone(),
            AudiencePermission::new("billing-service", "invoice:read"),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::UnknownPermission { .. }));

    // The explicit custom path works.
    authority
        .store
        .grant_custom(
            principal,
            AudiencePermission::new("invoice-service", "not-in-any-manifest"),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn duplicate_audiences_are_rejected() {
    let authority = authority().await;
    let err = authority
        .store
        .register_service(ServiceRegistration {
            service_id: "impostor".to_string(),
            display_name: "impostor".to_string(),
            audience: "invoice-service".to_string(),
            enabled: true,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::InvalidConfig(_)));
}

#[tokio::test]
async fn mutations_bump_authz_version() {
    let authority = authority().await;
    let before = authority.store.authz_version().await.unwrap();
    authority
        .store
        .grant(
            Principal::User {
                user_id: "alice".to_string(),
            },
            AudiencePermission::new("invoice-service", "invoice:write"),
        )
        .await
        .unwrap();
    assert!(authority.store.authz_version().await.unwrap() > before);
}

// --- Issuance ---

#[tokio::test]
async fn issuance_happy_path_validates_via_jwks() {
    let authority = authority().await;
    let issued = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap();

    let claims = invoice_validator(authority.issuer.jwks().await)
        .validate(&issued.token)
        .unwrap();
    assert_eq!(claims.sub, "billing-service");
    assert_eq!(claims.aud.as_deref(), Some("invoice-service"));
    assert_eq!(claims.permissions, vec!["invoice:read"]);
    assert_eq!(claims.token_type, TokenType::InternalService);
    assert_eq!(
        claims.authz_version,
        Some(authority.store.authz_version().await.unwrap())
    );
}

#[tokio::test]
async fn wrong_secret_fails_identity_verification() {
    let authority = authority().await;
    let mut request = token_request(&["invoice:read"]);
    request.proof = billing_proof("wrong-secret-that-is-32-bytes-long!");
    let err = authority
        .issuer
        .issue_internal_token(request)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::IdentityVerificationFailed { .. }));
}

#[tokio::test]
async fn unregistered_service_is_rejected_even_with_valid_proof() {
    let authority = authority().await;
    // ghost-service can prove identity but is not in the registry.
    authority
        .verifier
        .register("ghost-service", BILLING_SECRET.as_bytes())
        .await
        .unwrap();
    let request = InternalTokenRequest {
        proof: ServiceIdentityProof {
            service_id: "ghost-service".to_string(),
            proof: serde_json::json!({ "client_secret": BILLING_SECRET }),
        },
        audience: "invoice-service".to_string(),
        permissions: vec![],
    };
    let err = authority
        .issuer
        .issue_internal_token(request)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::UnknownService { .. }));
}

#[tokio::test]
async fn disabled_service_is_rejected() {
    let authority = authority().await;
    authority
        .store
        .set_service_enabled("billing-service", false)
        .await
        .unwrap();
    let err = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::ServiceDisabled { .. }));
}

#[tokio::test]
async fn unknown_audience_is_rejected() {
    let authority = authority().await;
    let mut request = token_request(&[]);
    request.audience = "no-such-service".to_string();
    let err = authority
        .issuer
        .issue_internal_token(request)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::UnknownAudience { .. }));
}

#[tokio::test]
async fn ungranted_permissions_are_rejected_with_missing_list() {
    let authority = authority().await;
    let err = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read", "invoice:write"]))
        .await
        .unwrap_err();
    let AuthzError::PermissionsNotGranted { missing, .. } = err else {
        panic!("expected PermissionsNotGranted");
    };
    assert_eq!(missing, vec!["invoice:write"]);
}

#[tokio::test]
async fn revoked_grant_denies_new_tokens() {
    let authority = authority().await;
    authority
        .store
        .revoke(
            &Principal::Service {
                service_id: "billing-service".to_string(),
            },
            &AudiencePermission::new("invoice-service", "invoice:read"),
        )
        .await
        .unwrap();
    let err = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::PermissionsNotGranted { .. }));
}

// --- Topology policy ---

#[tokio::test]
async fn loaded_policy_constrains_edges_and_permission_ceilings() {
    let authority = authority().await;
    // Grant billing invoice:write too, so only the policy constrains it.
    authority
        .store
        .grant(
            Principal::Service {
                service_id: "billing-service".to_string(),
            },
            AudiencePermission::new("invoice-service", "invoice:write"),
        )
        .await
        .unwrap();

    authority
        .issuer
        .load_policy(ServiceGraphPolicy {
            schema_version: 1,
            topology_name: "internal-tools".to_string(),
            policy_id: "internal-tools@1".to_string(),
            edges: vec![ServiceEdge {
                caller_service_id: "billing-service".to_string(),
                target_audience: "invoice-service".to_string(),
                permissions: BTreeSet::from(["invoice:read".to_string()]),
            }],
        })
        .await;

    // Within the edge: fine.
    authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap();

    // Granted but outside the edge ceiling: denied.
    let err = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:write"]))
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::PermissionsNotGranted { .. }));

    // Edge not declared at all: denied.
    let mut request = token_request(&[]);
    request.audience = "billing-service".to_string();
    let err = authority
        .issuer
        .issue_internal_token(request)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthzError::EdgeNotAllowed { .. }));
}

// --- Key rotation ---

#[tokio::test]
async fn rotation_keeps_outstanding_tokens_valid_and_removal_kills_them() {
    let authority = authority().await;
    let old_token = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap();

    authority
        .issuer
        .rotate_key(SigningKey::generate_es256("k2"))
        .await;
    let new_token = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap();

    let validator = invoice_validator(authority.issuer.jwks().await);
    assert!(validator.validate(&old_token.token).is_ok());
    assert!(validator.validate(&new_token.token).is_ok());

    assert!(authority.issuer.remove_retired_key("k1").await);
    let validator = invoice_validator(authority.issuer.jwks().await);
    assert!(validator.validate(&old_token.token).is_err());
    assert!(validator.validate(&new_token.token).is_ok());
}

// --- Audit ---

#[tokio::test]
async fn audit_records_outcomes_and_never_secrets() {
    let authority = authority().await;
    authority
        .issuer
        .issue_internal_token(token_request(&["invoice:read"]))
        .await
        .unwrap();
    let mut bad = token_request(&["invoice:read"]);
    bad.proof = billing_proof("wrong-secret-that-is-32-bytes-long!");
    let _ = authority.issuer.issue_internal_token(bad).await;
    let _ = authority
        .issuer
        .issue_internal_token(token_request(&["invoice:write"]))
        .await;

    let events = authority.audit.events().await;
    let kinds: Vec<_> = events.iter().map(|event| event.kind.clone()).collect();
    assert!(kinds.contains(&AuditEventKind::TokenIssued));
    assert!(kinds.contains(&AuditEventKind::IdentityVerificationFailed));
    assert!(kinds.contains(&AuditEventKind::TokenIssuanceDenied));

    // No secret or token material in any event.
    let serialized = serde_json::to_string(&events).unwrap();
    assert!(!serialized.contains(BILLING_SECRET));
    assert!(!serialized.contains("wrong-secret"));
    assert!(!serialized.contains("eyJ"), "JWTs must not be audited");
}

// --- Embedded router ---

#[tokio::test]
async fn authority_router_issues_and_serves_jwks() {
    let authority = authority().await;
    let app = authority_router(authority.issuer.clone());
    let server = axum_test::TestServer::new(app).unwrap();

    // JWKS endpoint.
    let jwks: JwkSet = server.get("/auth/jwks.json").await.json();
    assert_eq!(jwks.keys.len(), 1);

    // Issuance.
    let response = server
        .post("/auth/token")
        .json(&token_request(&["invoice:read"]))
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let token = body["token"].as_str().unwrap();
    let claims = invoice_validator(jwks).validate(token).unwrap();
    assert_eq!(claims.sub, "billing-service");

    // Identity failure -> 401 with a coarse error code.
    let mut bad = token_request(&[]);
    bad.proof = billing_proof("wrong-secret-that-is-32-bytes-long!");
    let response = server.post("/auth/token").json(&bad).await;
    response.assert_status(axum_test::http::StatusCode::UNAUTHORIZED);

    // Authorization failure -> 403.
    let response = server
        .post("/auth/token")
        .json(&token_request(&["invoice:write"]))
        .await;
    response.assert_status(axum_test::http::StatusCode::FORBIDDEN);
}
