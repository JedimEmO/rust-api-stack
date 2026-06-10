//! Gateway tests: narrowing semantics, derived-token caching, and the full
//! proxy path with header hygiene.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Duration;
use futures_util::StreamExt;
use ras_authorization_gateway::{
    AuthGateway, GatewayConfig, GatewayError, RouteRule, backend_validation_options, gateway_router,
};
use ras_authorization_token::{
    KeyResolver, KeyRing, RasClaims, SigningKey, TokenType, TokenValidator,
};
use ras_transport_core::http::{HeaderMap, StatusCode};
use ras_transport_core::{
    HttpTransport, RequestBody, TransportError, TransportRequest, TransportResponse,
    byte_stream_from,
};
use tokio::sync::Mutex;

const AUTH_ISSUER: &str = "https://auth.internal";
const GATEWAY_ISSUER: &str = "https://gateway.internal";

fn session_permissions() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        (
            "invoice-service".to_string(),
            vec!["invoice:read".to_string(), "invoice:approve".to_string()],
        ),
        (
            "billing-service".to_string(),
            vec!["billing:read".to_string()],
        ),
    ])
}

fn web_session(authority: &KeyRing, ttl: Duration) -> (String, RasClaims) {
    let claims = RasClaims::web_session(AUTH_ISSUER, "alice", session_permissions(), ttl)
        .with_authz_version(7);
    (authority.sign(&claims).unwrap(), claims)
}

fn routes() -> Vec<RouteRule> {
    vec![
        RouteRule::new("/invoices", "invoice-service", "http://invoice:3000"),
        RouteRule::new("/billing", "billing-service", "http://billing:3000"),
        RouteRule::new("/admin", "admin-service", "http://admin:3000"),
        RouteRule::new("/health", "health-service", "http://health:3000").authenticated_only(),
    ]
}

fn build_gateway(authority: &KeyRing) -> AuthGateway {
    AuthGateway::new(
        GatewayConfig::new(AUTH_ISSUER, GATEWAY_ISSUER, routes()),
        Arc::new(authority.jwks()) as Arc<dyn KeyResolver>,
        SigningKey::generate_es256("gw-1"),
    )
    .unwrap()
}

fn route(gateway: &AuthGateway, path: &str) -> RouteRule {
    gateway.routes().match_path(path).unwrap().clone()
}

// --- Narrowing semantics ---

#[test]
fn derived_token_is_single_audience_with_only_that_audiences_permissions() {
    let authority = KeyRing::new(SigningKey::generate_es256("auth-1"));
    let gateway = build_gateway(&authority);
    let (_, session) = web_session(&authority, Duration::minutes(30));

    let derived = gateway
        .derive_for_route(&session, &route(&gateway, "/invoices/1"))
        .unwrap();

    let validator = TokenValidator::new(
        gateway.jwks(),
        backend_validation_options(GATEWAY_ISSUER, "invoice-service"),
    );
    let claims = validator.validate(&derived.token).unwrap();
    assert_eq!(claims.token_type, TokenType::GatewayAccess);
    assert_eq!(claims.sub, "alice");
    assert_eq!(claims.aud.as_deref(), Some("invoice-service"));
    assert_eq!(claims.permissions, vec!["invoice:read", "invoice:approve"]);
    assert!(
        claims.audience_permissions.is_none(),
        "no cross-audience data"
    );
    assert_eq!(claims.authz_version, Some(7));

    // The same token must NOT validate for another audience.
    let wrong = TokenValidator::new(
        gateway.jwks(),
        backend_validation_options(GATEWAY_ISSUER, "billing-service"),
    );
    assert!(wrong.validate(&derived.token).is_err());
}

#[test]
fn missing_audience_permissions_fail_closed_unless_authenticated_only() {
    let authority = KeyRing::new(SigningKey::generate_es256("auth-1"));
    let gateway = build_gateway(&authority);
    let (_, session) = web_session(&authority, Duration::minutes(30));

    // No permissions for admin-service in the session.
    let err = gateway
        .derive_for_route(&session, &route(&gateway, "/admin"))
        .unwrap_err();
    assert!(matches!(
        err,
        GatewayError::NoPermissionsForAudience { audience } if audience == "admin-service"
    ));

    // Authenticated-only route: empty permission set is allowed.
    let derived = gateway
        .derive_for_route(&session, &route(&gateway, "/health"))
        .unwrap();
    let claims = TokenValidator::new(
        gateway.jwks(),
        backend_validation_options(GATEWAY_ISSUER, "health-service"),
    )
    .validate(&derived.token)
    .unwrap();
    assert!(claims.permissions.is_empty());
}

#[test]
fn derived_tokens_are_cached_per_session_audience_and_version() {
    let authority = KeyRing::new(SigningKey::generate_es256("auth-1"));
    let gateway = build_gateway(&authority);
    let (_, session) = web_session(&authority, Duration::minutes(30));

    let invoice_route = route(&gateway, "/invoices");
    let first = gateway.derive_for_route(&session, &invoice_route).unwrap();
    let second = gateway.derive_for_route(&session, &invoice_route).unwrap();
    assert_eq!(first.token, second.token, "cache reuses the derived token");

    // Different audience: different token.
    let billing = gateway
        .derive_for_route(&session, &route(&gateway, "/billing"))
        .unwrap();
    assert_ne!(first.token, billing.token);

    // Bumped authz version: cache miss, fresh token.
    let mut session_v8 = session.clone();
    session_v8.authz_version = Some(8);
    let rederived = gateway
        .derive_for_route(&session_v8, &invoice_route)
        .unwrap();
    assert_ne!(first.token, rederived.token);

    // Different session (new jti): cache miss.
    let (_, other_session) = web_session(&authority, Duration::minutes(30));
    let other = gateway
        .derive_for_route(&other_session, &invoice_route)
        .unwrap();
    assert_ne!(first.token, other.token);
}

#[test]
fn derived_token_never_outlives_the_session() {
    let authority = KeyRing::new(SigningKey::generate_es256("auth-1"));
    let gateway = build_gateway(&authority);
    // Session expiring in 30 seconds; derived TTL default is 2 minutes.
    let (_, session) = web_session(&authority, Duration::seconds(30));

    let derived = gateway
        .derive_for_route(&session, &route(&gateway, "/invoices"))
        .unwrap();
    assert!(derived.expires_at <= session.expires_at().unwrap());
}

#[test]
fn non_session_tokens_are_rejected_as_sessions() {
    let authority = KeyRing::new(SigningKey::generate_es256("auth-1"));
    let gateway = build_gateway(&authority);

    // An internal service token is not a web session.
    let internal = RasClaims::internal_service(
        AUTH_ISSUER,
        "billing-service",
        ras_authorization_token::PrincipalKind::Service,
        "invoice-service",
        vec!["invoice:read".to_string()],
        Duration::minutes(5),
    );
    let token = authority.sign(&internal).unwrap();
    assert!(matches!(
        gateway.validate_session(&token).unwrap_err(),
        GatewayError::InvalidSession(_)
    ));
    assert!(gateway.validate_session("garbage").is_err());
}

// --- Proxy path ---

struct Captured {
    url: String,
    method: String,
    authorization: Option<String>,
    cookie: Option<String>,
    connection: Option<String>,
    custom: Option<String>,
    body: Vec<u8>,
}

#[derive(Default)]
struct FakeUpstream {
    captured: Mutex<Vec<Captured>>,
}

#[async_trait]
impl HttpTransport for FakeUpstream {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let header = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let body = match request.body {
            RequestBody::Empty => Vec::new(),
            RequestBody::Bytes(bytes) => bytes.to_vec(),
            RequestBody::Stream(mut stream) => {
                let mut collected = Vec::new();
                while let Some(chunk) = stream.next().await {
                    collected.extend_from_slice(&chunk?);
                }
                collected
            }
        };
        self.captured.lock().await.push(Captured {
            url: request.url.clone(),
            method: request.method.to_string(),
            authorization: header("authorization"),
            cookie: header("cookie"),
            connection: header("connection"),
            custom: header("x-custom"),
            body,
        });

        let mut headers = HeaderMap::new();
        headers.insert("x-upstream", "yes".parse().unwrap());
        headers.insert("transfer-encoding", "chunked".parse().unwrap());
        Ok(TransportResponse::new(
            StatusCode::CREATED,
            headers,
            byte_stream_from(futures::stream::iter(vec![Ok(Bytes::from_static(
                b"upstream-body",
            ))])),
        ))
    }
}

struct ProxyFixture {
    server: axum_test::TestServer,
    upstream: Arc<FakeUpstream>,
    gateway: Arc<AuthGateway>,
    session_token: String,
}

fn proxy_fixture() -> ProxyFixture {
    let authority = KeyRing::new(SigningKey::generate_es256("auth-1"));
    let gateway = Arc::new(build_gateway(&authority));
    let upstream = Arc::new(FakeUpstream::default());
    let server =
        axum_test::TestServer::new(gateway_router(gateway.clone(), upstream.clone())).unwrap();
    let (session_token, _) = web_session(&authority, Duration::minutes(30));
    ProxyFixture {
        server,
        upstream,
        gateway,
        session_token,
    }
}

#[tokio::test]
async fn proxies_with_derived_bearer_and_strips_inbound_credentials() {
    let fixture = proxy_fixture();
    let response = fixture
        .server
        .post("/invoices/123")
        .add_query_param("verbose", "1")
        .authorization_bearer(&fixture.session_token)
        .add_header("cookie", "tracking=abc; ras_session=stale")
        .add_header("x-custom", "forwarded")
        .add_header("connection", "keep-alive")
        .bytes(Bytes::from_static(b"request-payload"))
        .await;

    response.assert_status(StatusCode::CREATED);
    assert_eq!(response.header("x-upstream"), "yes");
    response.assert_text("upstream-body");

    let captured = fixture.upstream.captured.lock().await;
    let request = &captured[0];
    assert_eq!(request.url, "http://invoice:3000/invoices/123?verbose=1");
    assert_eq!(request.method, "POST");
    assert_eq!(request.body, b"request-payload");
    assert_eq!(request.custom.as_deref(), Some("forwarded"));
    assert!(request.cookie.is_none(), "inbound cookies are stripped");
    assert!(request.connection.is_none(), "hop-by-hop headers stripped");

    // The backend got a derived token, not the session token.
    let bearer = request.authorization.as_deref().unwrap();
    let derived = bearer.strip_prefix("Bearer ").unwrap();
    assert_ne!(derived, fixture.session_token);
    let claims = TokenValidator::new(
        fixture.gateway.jwks(),
        backend_validation_options(GATEWAY_ISSUER, "invoice-service"),
    )
    .validate(derived)
    .unwrap();
    assert_eq!(claims.permissions, vec!["invoice:read", "invoice:approve"]);
}

#[tokio::test]
async fn cookie_sessions_work_and_are_not_forwarded() {
    let fixture = proxy_fixture();
    let response = fixture
        .server
        .get("/billing/summary")
        .add_header(
            "cookie",
            format!("ras_session={}; theme=dark", fixture.session_token),
        )
        .await;
    response.assert_status(StatusCode::CREATED);

    let captured = fixture.upstream.captured.lock().await;
    assert!(captured[0].cookie.is_none());
    let bearer = captured[0].authorization.as_deref().unwrap();
    let claims = TokenValidator::new(
        fixture.gateway.jwks(),
        backend_validation_options(GATEWAY_ISSUER, "billing-service"),
    )
    .validate(bearer.strip_prefix("Bearer ").unwrap())
    .unwrap();
    assert_eq!(claims.permissions, vec!["billing:read"]);
}

#[tokio::test]
async fn failure_modes_map_to_correct_statuses() {
    let fixture = proxy_fixture();

    // Unmatched route: 404, nothing proxied, even with a valid session.
    fixture
        .server
        .get("/unknown")
        .authorization_bearer(&fixture.session_token)
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // No session: 401.
    fixture
        .server
        .get("/invoices")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    // Garbage session: 401.
    fixture
        .server
        .get("/invoices")
        .authorization_bearer("garbage")
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    // Valid session but no permissions for the route audience: 403.
    fixture
        .server
        .get("/admin/users")
        .authorization_bearer(&fixture.session_token)
        .await
        .assert_status(StatusCode::FORBIDDEN);

    // Connection upgrades fail closed: 501.
    fixture
        .server
        .get("/invoices")
        .authorization_bearer(&fixture.session_token)
        .add_header("upgrade", "websocket")
        .await
        .assert_status(StatusCode::NOT_IMPLEMENTED);

    // Authenticated-only route with no audience permissions: proxied.
    fixture
        .server
        .get("/health")
        .authorization_bearer(&fixture.session_token)
        .await
        .assert_status(StatusCode::CREATED);

    assert_eq!(
        fixture.upstream.captured.lock().await.len(),
        1,
        "only the authenticated-only request reached the upstream"
    );
}

#[tokio::test]
async fn key_rotation_keeps_outstanding_derived_tokens_valid() {
    let fixture = proxy_fixture();
    fixture
        .server
        .get("/invoices")
        .authorization_bearer(&fixture.session_token)
        .await
        .assert_status(StatusCode::CREATED);

    let captured = fixture.upstream.captured.lock().await;
    let old_token = captured[0]
        .authorization
        .as_deref()
        .unwrap()
        .strip_prefix("Bearer ")
        .unwrap()
        .to_string();
    drop(captured);

    fixture
        .gateway
        .rotate_key(SigningKey::generate_es256("gw-2"));
    let validator = TokenValidator::new(
        fixture.gateway.jwks(),
        backend_validation_options(GATEWAY_ISSUER, "invoice-service"),
    );
    assert!(validator.validate(&old_token).is_ok());
}
