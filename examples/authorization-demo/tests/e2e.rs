//! Full-stack integration test: browser session → gateway → generated RAS
//! services, with billing calling invoice through the embedded authority —
//! all in-process, no sockets.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use authorization_demo::authorization_demo_topology;
use authorization_demo::demo::{
    AUTHORITY_ISSUER, Authority, GATEWAY_ISSUER, build_authority, build_billing_router,
    build_invoice_router,
};
use ras_authorization_core::AuditEventKind;
use ras_authorization_gateway::{AuthGateway, GatewayConfig, gateway_router};
use ras_authorization_token::{KeyResolver, KeyRing, RasClaims, SigningKey};
use ras_integration_core::IntegrationError;
use ras_integration_ras::{EmbeddedAuthority, RasInternalTokenSource};
use ras_transport_core::{
    AxumTestTransport, HttpTransport, TransportError, TransportRequest, TransportResponse,
};

const INVOICE_UPSTREAM: &str = "http://invoice-service:3000";
const BILLING_UPSTREAM: &str = "http://billing-service:3000";

/// Routes proxied requests to in-process services by URL authority.
struct HostRoutingTransport {
    routes: HashMap<String, AxumTestTransport>,
}

#[async_trait]
impl HttpTransport for HostRoutingTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let authority = request
            .url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or_default()
            .to_string();
        let transport = self
            .routes
            .get(&authority)
            .ok_or_else(|| TransportError::Connection(format!("no upstream for {authority}")))?;
        transport.execute(request).await
    }
}

struct Stack {
    authority: Authority,
    session_keys: KeyRing,
    gateway: axum_test::TestServer,
    invoice_direct: Arc<axum_test::TestServer>,
}

async fn build_stack() -> Stack {
    let topology = authorization_demo_topology().expect("topology validates");
    let authority = build_authority().await;
    let authority_jwks = authority.issuer.jwks().await;

    let session_keys = KeyRing::new(SigningKey::generate_es256("session-1"));

    // Gateway from the generated profile + in-process upstream bindings.
    let profile = topology.gateway_profile_toml("public_web").unwrap();
    let upstreams = BTreeMap::from([
        ("invoice-service".to_string(), INVOICE_UPSTREAM.to_string()),
        ("billing-service".to_string(), BILLING_UPSTREAM.to_string()),
    ]);
    let gateway = Arc::new(
        AuthGateway::new(
            GatewayConfig::from_profile_toml(
                AUTHORITY_ISSUER,
                GATEWAY_ISSUER,
                &profile,
                &upstreams,
            )
            .unwrap(),
            Arc::new(session_keys.jwks()) as Arc<dyn KeyResolver>,
            SigningKey::generate_es256("gateway-1"),
        )
        .unwrap(),
    );
    let gateway_jwks = gateway.jwks();

    // Invoice service (accepts internal + gateway tokens).
    let invoice_server = Arc::new(
        axum_test::TestServer::new(build_invoice_router(authority_jwks, gateway_jwks.clone()))
            .unwrap(),
    );
    let invoice_transport = AxumTestTransport::from_arc(invoice_server.clone());

    // Billing service (accepts gateway tokens; calls invoice internally).
    let billing_router = build_billing_router(
        &authority,
        gateway_jwks,
        INVOICE_UPSTREAM,
        Arc::new(invoice_transport.clone()),
    );
    let billing_server = axum_test::TestServer::new(billing_router).unwrap();

    // Gateway proxying to both in-process services.
    let upstream = Arc::new(HostRoutingTransport {
        routes: HashMap::from([
            ("invoice-service:3000".to_string(), invoice_transport),
            (
                "billing-service:3000".to_string(),
                AxumTestTransport::new(billing_server),
            ),
        ]),
    });
    let gateway_server = axum_test::TestServer::new(gateway_router(gateway, upstream)).unwrap();

    Stack {
        authority,
        session_keys,
        gateway: gateway_server,
        invoice_direct: invoice_server,
    }
}

fn session(stack: &Stack, permissions: &[(&str, &[&str])]) -> String {
    let audience_permissions = permissions
        .iter()
        .map(|(audience, permissions)| {
            (
                audience.to_string(),
                permissions.iter().map(|p| p.to_string()).collect(),
            )
        })
        .collect();
    let claims = RasClaims::web_session(
        AUTHORITY_ISSUER,
        "alice",
        audience_permissions,
        chrono::Duration::minutes(30),
    );
    stack.session_keys.sign(&claims).unwrap()
}

#[tokio::test]
async fn browser_reads_invoices_through_the_gateway() {
    let stack = build_stack().await;
    let token = session(&stack, &[("invoice-service", &["invoice:read"])]);

    let response = stack
        .gateway
        .get("/api/invoice/invoices")
        .authorization_bearer(&token)
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert_eq!(body["invoices"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn billing_summary_calls_invoice_with_an_internal_token() {
    let stack = build_stack().await;
    let token = session(&stack, &[("billing-service", &["billing:read"])]);

    let response = stack
        .gateway
        .get("/api/billing/summary")
        .authorization_bearer(&token)
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    assert_eq!(body["invoice_count"], 2);
    assert_eq!(body["total_cents"], 11150);

    // The internal billing -> invoice call went through the authority.
    let events = stack.authority.audit.events().await;
    assert!(
        events
            .iter()
            .any(|event| event.kind == AuditEventKind::TokenIssued
                && event.actor.as_deref() == Some("billing")
                && event.target.as_deref() == Some("invoice-service"))
    );
}

#[tokio::test]
async fn sessions_without_audience_permissions_fail_closed_at_the_gateway() {
    let stack = build_stack().await;
    // Session with only billing permissions cannot reach invoice routes.
    let token = session(&stack, &[("billing-service", &["billing:read"])]);
    stack
        .gateway
        .get("/api/invoice/invoices")
        .authorization_bearer(&token)
        .await
        .assert_status(axum_test::http::StatusCode::FORBIDDEN);

    // No session at all: 401. Unknown route: 404.
    stack
        .gateway
        .get("/api/invoice/invoices")
        .await
        .assert_status(axum_test::http::StatusCode::UNAUTHORIZED);
    stack
        .gateway
        .get("/api/unknown")
        .authorization_bearer(&token)
        .await
        .assert_status(axum_test::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn narrowed_tokens_enforce_per_operation_permissions_downstream() {
    let stack = build_stack().await;
    // The session can read invoices but not write them. The gateway
    // forwards (the audience has permissions), and the generated service
    // enforces the per-operation requirement.
    let token = session(&stack, &[("invoice-service", &["invoice:read"])]);

    stack
        .gateway
        .get("/api/invoice/invoices")
        .authorization_bearer(&token)
        .await
        .assert_status_ok();

    stack
        .gateway
        .post("/api/invoice/invoices")
        .authorization_bearer(&token)
        .json(&serde_json::json!({"customer": "initech", "amount_cents": 100}))
        .await
        .assert_status(axum_test::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn web_session_tokens_are_rejected_by_backends_directly() {
    let stack = build_stack().await;
    let token = session(
        &stack,
        &[("invoice-service", &["invoice:read", "invoice:write"])],
    );

    // Bypassing the gateway with the multi-audience session token fails:
    // backends only accept single-audience RAS tokens.
    stack
        .invoice_direct
        .get("/api/invoice/invoices")
        .authorization_bearer(&token)
        .await
        .assert_status(axum_test::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn undeclared_topology_edges_are_denied_by_the_authority() {
    let stack = build_stack().await;
    // invoice -> billing is not a declared edge (and has no grant): the
    // authority refuses even though the service identity is valid.
    stack
        .authority
        .verifier
        .register("invoice", "invoice-demo-secret-32-bytes-long!!!".as_bytes())
        .await
        .unwrap();
    let source = RasInternalTokenSource::new(
        Arc::new(EmbeddedAuthority::new(stack.authority.issuer.clone())),
        ras_authorization_core::ServiceIdentityProof {
            service_id: "invoice".to_string(),
            proof: serde_json::json!({ "client_secret": "invoice-demo-secret-32-bytes-long!!!" }),
        },
    );
    let err = ras_integration_core::TokenSource::issue_token(
        &source,
        &ras_integration_core::TokenRequest {
            integration_id: "billing-service".to_string(),
            subject: ras_integration_core::TokenSubject::Service,
            scopes: vec![],
            audience: Some("billing-service".to_string()),
            force_refresh: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, IntegrationError::Denied { .. }));
}
