//! RasInternalTokenSource tests: embedded and HTTP authority modes, plus an
//! end-to-end service-to-service flow through the token manager and a
//! capability-scoped client.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use ras_authorization_core::{
    AudiencePermission, InMemoryAuditSink, InMemoryAuthorizationStore, Principal,
    ServiceIdentityProof, ServiceRegistration, StaticSecretVerifier, TokenIssuer,
};
use ras_authorization_token::{
    AudiencePolicy, SigningKey, TokenType, TokenValidator, ValidationOptions,
};
use ras_integration_core::{
    AuthorizedHttpClient, IntegrationConfig, IntegrationError, TokenManager, TokenRequest,
    TokenSource, TokenSubject,
};
use ras_integration_ras::{EmbeddedAuthority, HttpAuthority, RasInternalTokenSource};
use ras_permission_manifest::{
    AuthRequirementInfo, OperationKind, OperationPermissions, PermissionManifest,
    ServicePermissions, TransportKind, WireTarget,
};
use ras_transport_core::http::{HeaderMap, StatusCode};
use ras_transport_core::{
    HttpTransport, TransportError, TransportRequest, TransportResponse, byte_stream_from,
};
use tokio::sync::Mutex;

const ISSUER: &str = "https://auth.internal";
const BILLING_SECRET: &str = "billing-service-static-secret-32b!!";

fn invoice_manifest() -> PermissionManifest {
    let operations = ["invoice:read", "invoice:write"]
        .iter()
        .map(|permission| OperationPermissions {
            operation_id: format!("op_{permission}"),
            operation_name: format!("op_{permission}"),
            kind: OperationKind::RestEndpoint,
            wire: WireTarget::Rest {
                method: "GET".to_string(),
                path: "/invoices".to_string(),
            },
            auth: AuthRequirementInfo::from_permission_groups([[*permission]]),
            version: None,
            canonical_operation_id: None,
        })
        .collect();
    PermissionManifest::from_services([ServicePermissions {
        service_name: "InvoiceService".to_string(),
        transport: TransportKind::Rest,
        operations,
    }])
}

struct Fixture {
    issuer: Arc<TokenIssuer>,
    audit: Arc<InMemoryAuditSink>,
}

async fn authority_fixture() -> Fixture {
    let store = Arc::new(InMemoryAuthorizationStore::new());
    let verifier = Arc::new(StaticSecretVerifier::new());
    let audit = Arc::new(InMemoryAuditSink::new());

    for id in ["billing-service", "invoice-service"] {
        store
            .register_service(ServiceRegistration {
                service_id: id.to_string(),
                display_name: id.to_string(),
                audience: id.to_string(),
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
        .import_manifest("invoice-service", &invoice_manifest())
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
    Fixture { issuer, audit }
}

fn billing_proof() -> ServiceIdentityProof {
    ServiceIdentityProof {
        service_id: "billing-service".to_string(),
        proof: serde_json::json!({ "client_secret": BILLING_SECRET }),
    }
}

fn internal_request(scopes: &[&str]) -> TokenRequest {
    TokenRequest {
        integration_id: "invoice-service".to_string(),
        subject: TokenSubject::Service,
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        audience: Some("invoice-service".to_string()),
        force_refresh: false,
    }
}

#[tokio::test]
async fn embedded_authority_issues_validatable_internal_tokens() {
    let fixture = authority_fixture().await;
    let source = RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(fixture.issuer.clone())),
        billing_proof(),
    );

    let lease = source
        .issue_token(&internal_request(&["invoice:read"]))
        .await
        .unwrap();
    assert!(lease.expires_at.is_some());

    let validator = TokenValidator::new(
        fixture.issuer.jwks().await,
        ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::InternalService],
        ),
    );
    let claims = validator
        .validate(lease.access_token.expose_secret())
        .unwrap();
    assert_eq!(claims.sub, "billing-service");
    assert_eq!(claims.permissions, vec!["invoice:read"]);
}

#[tokio::test]
async fn authority_denial_surfaces_and_no_token_is_produced() {
    let fixture = authority_fixture().await;
    let source = RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(fixture.issuer.clone())),
        billing_proof(),
    );

    let err = source
        .issue_token(&internal_request(&["invoice:write"]))
        .await
        .unwrap_err();
    assert!(
        matches!(err, IntegrationError::Denied { reason, .. } if reason.contains("invoice:write"))
    );
}

#[tokio::test]
async fn identity_failure_surfaces_as_denied() {
    let fixture = authority_fixture().await;
    let source = RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(fixture.issuer.clone())),
        ServiceIdentityProof {
            service_id: "billing-service".to_string(),
            proof: serde_json::json!({ "client_secret": "wrong-secret-that-is-32-bytes!!!" }),
        },
    );
    let err = source
        .issue_token(&internal_request(&["invoice:read"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Denied { .. }));
}

#[tokio::test]
async fn non_service_subjects_fail_closed_before_any_authority_call() {
    let fixture = authority_fixture().await;
    let source = RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(fixture.issuer.clone())),
        billing_proof(),
    );

    for subject in [
        TokenSubject::User {
            user_id: "alice".to_string(),
        },
        TokenSubject::ServiceAccount {
            service_account_id: "bot".to_string(),
        },
    ] {
        let mut request = internal_request(&["invoice:read"]);
        request.subject = subject;
        let err = source.issue_token(&request).await.unwrap_err();
        assert!(matches!(err, IntegrationError::Denied { .. }));
    }
    // The authority never saw a request: no audit events were recorded.
    assert!(fixture.audit.events().await.is_empty());
}

#[tokio::test]
async fn missing_audience_fails_closed() {
    let fixture = authority_fixture().await;
    let source = RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(fixture.issuer.clone())),
        billing_proof(),
    );
    let mut request = internal_request(&[]);
    request.audience = None;
    let err = source.issue_token(&request).await.unwrap_err();
    assert!(matches!(err, IntegrationError::InvalidConfig(_)));
}

// --- HTTP authority mode ---

#[derive(Default)]
struct ScriptedAuthority {
    responses: Mutex<Vec<(StatusCode, serde_json::Value)>>,
    bodies: Mutex<Vec<serde_json::Value>>,
}

#[async_trait]
impl HttpTransport for ScriptedAuthority {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        if let ras_transport_core::RequestBody::Bytes(bytes) = &request.body {
            self.bodies
                .lock()
                .await
                .push(serde_json::from_slice(bytes).unwrap());
        }
        let (status, body) = self.responses.lock().await.remove(0);
        Ok(TransportResponse::new(
            status,
            HeaderMap::new(),
            byte_stream_from(futures::stream::iter(vec![Ok(Bytes::from(
                serde_json::to_vec(&body).unwrap(),
            ))])),
        ))
    }
}

#[tokio::test]
async fn http_authority_round_trip_and_error_mapping() {
    let transport = Arc::new(ScriptedAuthority::default());
    transport.responses.lock().await.extend([
        (
            StatusCode::OK,
            serde_json::json!({
                "token": "signed.jwt.value",
                "expires_at": chrono::Utc::now() + chrono::Duration::minutes(5)
            }),
        ),
        (
            StatusCode::FORBIDDEN,
            serde_json::json!({"error": "issuance_denied"}),
        ),
        (StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({})),
        (StatusCode::OK, serde_json::json!({"unexpected": true})),
    ]);

    let source = RasInternalTokenSource::new(
        Arc::new(HttpAuthority::new(
            transport.clone(),
            "https://auth.internal/auth/token",
        )),
        billing_proof(),
    );

    // 200 -> lease.
    let lease = source
        .issue_token(&internal_request(&["invoice:read"]))
        .await
        .unwrap();
    assert_eq!(lease.access_token.expose_secret(), "signed.jwt.value");

    // The posted body is a full InternalTokenRequest.
    let bodies = transport.bodies.lock().await.clone();
    assert_eq!(bodies[0]["proof"]["service_id"], "billing-service");
    assert_eq!(bodies[0]["audience"], "invoice-service");
    assert_eq!(bodies[0]["permissions"][0], "invoice:read");

    // 403 -> Denied with the authority's error code.
    let err = source
        .issue_token(&internal_request(&["invoice:read"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Denied { reason, .. } if reason == "issuance_denied"));

    // 500 -> Provider.
    let err = source
        .issue_token(&internal_request(&["invoice:read"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Provider { .. }));

    // Malformed 200 -> Provider.
    let err = source
        .issue_token(&internal_request(&["invoice:read"]))
        .await
        .unwrap_err();
    assert!(matches!(err, IntegrationError::Provider { .. }));
}

// --- End to end: capability client -> manager -> internal source -> JWKS ---

#[derive(Default)]
struct CapturingBackend {
    authorization: Mutex<Vec<String>>,
}

#[async_trait]
impl HttpTransport for CapturingBackend {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let auth = request
            .headers
            .get(ras_transport_core::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        self.authorization.lock().await.push(auth);
        Ok(TransportResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            byte_stream_from(futures::stream::iter(vec![Ok(Bytes::from_static(b"[]"))])),
        ))
    }
}

#[tokio::test]
async fn billing_calls_invoice_with_authority_issued_bearer() {
    let fixture = authority_fixture().await;
    let source = Arc::new(RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(fixture.issuer.clone())),
        billing_proof(),
    ));

    let manager = Arc::new(
        TokenManager::builder()
            .register(
                IntegrationConfig::new(
                    "invoice-service",
                    ["invoice:read"],
                    ["http://invoice-service:3000"],
                )
                .unwrap()
                .with_allowed_audiences(["invoice-service"]),
                source,
            )
            .unwrap()
            .build(),
    );

    let backend = Arc::new(CapturingBackend::default());
    let client = AuthorizedHttpClient::for_service(
        backend.clone(),
        manager,
        "invoice-service",
        ["invoice:read"],
    )
    .with_audience("invoice-service");

    client
        .get("http://invoice-service:3000/api/invoices")
        .await
        .unwrap();

    // The backend received a bearer that validates against the authority's
    // JWKS as a single-audience internal token.
    let seen = backend.authorization.lock().await.clone();
    let token = seen[0].strip_prefix("Bearer ").unwrap();
    let validator = TokenValidator::new(
        fixture.issuer.jwks().await,
        ValidationOptions::new(
            ISSUER,
            AudiencePolicy::Exact("invoice-service".to_string()),
            vec![TokenType::InternalService],
        ),
    );
    let claims = validator.validate(token).unwrap();
    assert_eq!(claims.sub, "billing-service");
    assert_eq!(claims.permissions, vec!["invoice:read"]);
    assert!(claims.audience_permissions.is_none());
}
