//! Run the full demo stack on localhost: embedded authority, invoice and
//! billing services, and the auth gateway in front.
//!
//! ```text
//! cargo run -p authorization-demo
//! curl -H "Authorization: Bearer <printed session token>" \
//!     http://127.0.0.1:8080/api/invoice/invoices
//! curl -H "Authorization: Bearer <printed session token>" \
//!     http://127.0.0.1:8080/api/billing/summary
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use authorization_demo::authorization_demo_topology;
use authorization_demo::demo::{
    AUTHORITY_ISSUER, GATEWAY_ISSUER, build_authority, build_billing_router, build_invoice_router,
};
use ras_authorization_gateway::{AuthGateway, GatewayConfig, gateway_router};
use ras_authorization_token::{KeyResolver, KeyRing, RasClaims, SigningKey};
use ras_transport_core::ReqwestTransport;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let topology = authorization_demo_topology().expect("topology validates");
    println!("--- topology diagram (mermaid) ---\n{}", topology.mermaid());

    let authority = build_authority().await;
    let authority_jwks = authority.issuer.jwks().await;

    // Web sessions: in this demo the session authority is a local keyring.
    let session_keys = KeyRing::new(SigningKey::generate_es256("session-1"));
    let session = RasClaims::web_session(
        AUTHORITY_ISSUER,
        "alice",
        BTreeMap::from([
            (
                "invoice-service".to_string(),
                vec!["invoice:read".to_string(), "invoice:write".to_string()],
            ),
            (
                "billing-service".to_string(),
                vec!["billing:read".to_string()],
            ),
        ]),
        chrono::Duration::hours(1),
    );
    let session_token = session_keys.sign(&session).expect("session signs");

    // Gateway config from the generated topology profile + deployment
    // upstream bindings.
    let profile = topology
        .gateway_profile_toml("public_web")
        .expect("gateway profile");
    let upstreams = BTreeMap::from([
        (
            "invoice-service".to_string(),
            "http://127.0.0.1:8081".to_string(),
        ),
        (
            "billing-service".to_string(),
            "http://127.0.0.1:8082".to_string(),
        ),
    ]);
    let gateway_config =
        GatewayConfig::from_profile_toml(AUTHORITY_ISSUER, GATEWAY_ISSUER, &profile, &upstreams)
            .expect("gateway config");
    let gateway = Arc::new(
        AuthGateway::new(
            gateway_config,
            Arc::new(session_keys.jwks()) as Arc<dyn KeyResolver>,
            SigningKey::generate_es256("gateway-1"),
        )
        .expect("gateway"),
    );
    let gateway_jwks = gateway.jwks();

    let invoice_router = build_invoice_router(authority_jwks, gateway_jwks.clone());
    let billing_router = build_billing_router(
        &authority,
        gateway_jwks,
        "http://127.0.0.1:8081",
        Arc::new(ReqwestTransport::new()),
    );
    let gateway_app = gateway_router(gateway, Arc::new(ReqwestTransport::new()));

    let invoice_listener = tokio::net::TcpListener::bind("127.0.0.1:8081")
        .await
        .expect("bind invoice");
    let billing_listener = tokio::net::TcpListener::bind("127.0.0.1:8082")
        .await
        .expect("bind billing");
    let gateway_listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("bind gateway");

    println!("--- demo session token (alice) ---\n{session_token}\n");
    println!("gateway:  http://127.0.0.1:8080  (routes /api/invoice, /api/billing)");
    println!("invoice:  http://127.0.0.1:8081  (direct access requires RAS tokens)");
    println!("billing:  http://127.0.0.1:8082");

    let invoice = tokio::spawn(async move {
        axum::serve(invoice_listener, invoice_router).await.unwrap();
    });
    let billing = tokio::spawn(async move {
        axum::serve(billing_listener, billing_router).await.unwrap();
    });
    let gateway = tokio::spawn(async move {
        axum::serve(gateway_listener, gateway_app).await.unwrap();
    });

    let _ = tokio::try_join!(invoice, billing, gateway);
}
