//! Regression tests for the `rest_service!` hardening prompted by the XM
//! device-integration feedback:
//!
//! * Content-Type enforcement (strict `application/json`, opt-out).
//! * `413` vs `400` split for over-limit vs unreadable bodies.
//! * `204 No Content` no longer carries a serialized body.
//! * Per-endpoint `body_limit` override.
//! * Opt-in request-header parameter for handlers.
//! * Startup assertion when a permissioned service has no auth provider.
//! * `docs_require_auth` gate on the docs / openapi routes.
//!
//! Each of these fails against the pre-hardening macro.

use axum_test::TestServer;
use ras_auth_core::{AuthError, AuthProvider, AuthenticatedUser};
use ras_rest_core::{RestResponse, RestResult};
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::pin::Pin;

#[derive(Clone)]
struct MockAuth;

impl AuthProvider for MockAuth {
    fn authenticate(
        &self,
        authorization: String,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<AuthenticatedUser, AuthError>> + Send + '_>>
    {
        Box::pin(async move {
            if authorization == "admin-token" {
                let mut permissions = HashSet::new();
                permissions.insert("admin".to_string());
                Ok(AuthenticatedUser {
                    user_id: "admin".to_string(),
                    permissions,
                    metadata: None,
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        })
    }

    fn check_permissions(
        &self,
        user: &AuthenticatedUser,
        required_permissions: &[String],
    ) -> Result<(), AuthError> {
        if required_permissions
            .iter()
            .all(|perm| user.permissions.contains(perm))
        {
            Ok(())
        } else {
            Err(AuthError::InsufficientPermissions {
                required: required_permissions.to_vec(),
                has: user.permissions.iter().cloned().collect(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Content-Type enforcement (strict default) + opt-out
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Payload {
    value: String,
}

rest_service!({
    service_name: StrictService,
    base_path: "/strict",
    endpoints: [
        POST UNAUTHORIZED submit(Payload) -> Value,
    ]
});

struct StrictImpl;

#[async_trait::async_trait]
impl StrictServiceTrait for StrictImpl {
    async fn post_submit(&self, request: Payload) -> RestResult<Value> {
        Ok(RestResponse::ok(json!({ "echo": request.value })))
    }
}

rest_service!({
    service_name: LenientService,
    base_path: "/lenient",
    require_json_content_type: false,
    endpoints: [
        POST UNAUTHORIZED submit(Payload) -> Value,
    ]
});

struct LenientImpl;

#[async_trait::async_trait]
impl LenientServiceTrait for LenientImpl {
    async fn post_submit(&self, request: Payload) -> RestResult<Value> {
        Ok(RestResponse::ok(json!({ "echo": request.value })))
    }
}

#[tokio::test]
async fn strict_service_requires_json_content_type() {
    let app = StrictServiceBuilder::new(StrictImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    // application/json → accepted.
    let ok = server
        .post("/strict/submit")
        .json(&json!({ "value": "hi" }))
        .await;
    assert_eq!(ok.status_code().as_u16(), 200);

    // text/plain carrying JSON-parseable bytes → 415, before the body is parsed.
    // (text/plain is CORS-safelisted, so this is the simple-request CSRF shape.)
    let rejected = server
        .post("/strict/submit")
        .text(json!({ "value": "hi" }).to_string())
        .content_type("text/plain")
        .await;
    assert_eq!(rejected.status_code().as_u16(), 415);

    // No Content-Type at all → 415.
    let missing = server
        .post("/strict/submit")
        .bytes(json!({ "value": "hi" }).to_string().into_bytes().into())
        .content_type("")
        .await;
    assert_eq!(missing.status_code().as_u16(), 415);
}

#[tokio::test]
async fn strict_service_accepts_json_with_charset_parameter() {
    let app = StrictServiceBuilder::new(StrictImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let ok = server
        .post("/strict/submit")
        .text(json!({ "value": "hi" }).to_string())
        .content_type("application/json; charset=utf-8")
        .await;
    assert_eq!(ok.status_code().as_u16(), 200);
}

#[tokio::test]
async fn lenient_service_accepts_any_content_type() {
    let app = LenientServiceBuilder::new(LenientImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let ok = server
        .post("/lenient/submit")
        .text(json!({ "value": "hi" }).to_string())
        .content_type("text/plain")
        .await;
    assert_eq!(ok.status_code().as_u16(), 200);
}

// ---------------------------------------------------------------------------
// 204 No Content must not carry a body
// ---------------------------------------------------------------------------

rest_service!({
    service_name: NoContentService,
    base_path: "/nc",
    endpoints: [
        DELETE UNAUTHORIZED thing() -> (),
    ]
});

struct NoContentImpl;

#[async_trait::async_trait]
impl NoContentServiceTrait for NoContentImpl {
    async fn delete_thing(&self) -> RestResult<()> {
        Ok(RestResponse::no_content())
    }
}

#[tokio::test]
async fn no_content_response_has_empty_body() {
    let app = NoContentServiceBuilder::new(NoContentImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.delete("/nc/thing").await;
    assert_eq!(response.status_code().as_u16(), 204);
    // Before the fix this carried a serialized `null` body.
    assert!(
        response.as_bytes().is_empty(),
        "204 response should have no body, got {:?}",
        response.as_bytes()
    );
}

// A 204 on a NON-unit response type must also emit an empty body — the case
// that broke the generated client (it deserialized the empty body as EOF).
rest_service!({
    service_name: MaybeService,
    base_path: "/maybe",
    endpoints: [
        GET UNAUTHORIZED maybe() -> Option<Value>,
    ]
});

struct MaybeImpl;

#[async_trait::async_trait]
impl MaybeServiceTrait for MaybeImpl {
    async fn get_maybe(&self) -> RestResult<Option<Value>> {
        Ok(RestResponse::no_content())
    }
}

#[tokio::test]
async fn no_content_with_non_unit_response_type_has_empty_body() {
    let app = MaybeServiceBuilder::new(MaybeImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.get("/maybe/maybe").await;
    assert_eq!(response.status_code().as_u16(), 204);
    assert!(
        response.as_bytes().is_empty(),
        "204 with Option<Value> response should still have no body, got {:?}",
        response.as_bytes()
    );
}

// The 413 Content-Length precheck (distinct from the to_bytes fallback) fires
// only when a Content-Length header is present. A real HTTP transport sets it,
// so this exercises the precheck path that mock_transport never reaches.
rest_service!({
    service_name: ClLimitService,
    base_path: "/cl",
    body_limit: 64,
    endpoints: [
        POST UNAUTHORIZED echo(Value) -> Value,
    ]
});

struct ClLimitImpl;

#[async_trait::async_trait]
impl ClLimitServiceTrait for ClLimitImpl {
    async fn post_echo(&self, request: Value) -> RestResult<Value> {
        Ok(RestResponse::ok(request))
    }
}

#[tokio::test]
async fn content_length_precheck_returns_413() {
    let app = ClLimitServiceBuilder::new(ClLimitImpl).build();
    let server = TestServer::builder().http_transport().build(app).unwrap();

    let response = server
        .post("/cl/echo")
        .json(&json!({ "data": "x".repeat(256) }))
        .await;
    assert_eq!(response.status_code().as_u16(), 413);
}

// ---------------------------------------------------------------------------
// Per-endpoint body_limit override
// ---------------------------------------------------------------------------

rest_service!({
    service_name: PerEndpointLimitService,
    base_path: "/pel",
    body_limit: 1048576,
    endpoints: [
        POST UNAUTHORIZED small(Value) -> Value { body_limit: 16 },
    ]
});

struct PerEndpointLimitImpl;

#[async_trait::async_trait]
impl PerEndpointLimitServiceTrait for PerEndpointLimitImpl {
    async fn post_small(&self, request: Value) -> RestResult<Value> {
        Ok(RestResponse::ok(request))
    }
}

#[tokio::test]
async fn per_endpoint_body_limit_overrides_service_limit() {
    let app = PerEndpointLimitServiceBuilder::new(PerEndpointLimitImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    // Well over the 16-byte endpoint cap (but under the 1 MiB service cap).
    let response = server
        .post("/pel/small")
        .json(&json!({ "data": "x".repeat(64) }))
        .await;
    assert_eq!(response.status_code().as_u16(), 413);
}

// ---------------------------------------------------------------------------
// Opt-in request headers in the handler signature
// ---------------------------------------------------------------------------

rest_service!({
    service_name: HeaderService,
    base_path: "/hdr",
    endpoints: [
        POST UNAUTHORIZED echo(Payload) -> Value { headers: true },
    ]
});

struct HeaderImpl;

#[async_trait::async_trait]
impl HeaderServiceTrait for HeaderImpl {
    async fn post_echo(
        &self,
        headers: axum::http::HeaderMap,
        request: Payload,
    ) -> RestResult<Value> {
        let device = headers
            .get("x-device-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("none")
            .to_string();
        Ok(RestResponse::ok(json!({
            "device": device,
            "value": request.value,
        })))
    }
}

#[tokio::test]
async fn handler_receives_opt_in_headers() {
    let app = HeaderServiceBuilder::new(HeaderImpl).build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server
        .post("/hdr/echo")
        .add_header("x-device-id", "chassis-42")
        .json(&json!({ "value": "ping" }))
        .await;
    assert_eq!(response.status_code().as_u16(), 200);
    let body: Value = response.json();
    assert_eq!(body["device"], "chassis-42");
    assert_eq!(body["value"], "ping");
}

// ---------------------------------------------------------------------------
// Startup assertion: permissioned service without an auth provider
// ---------------------------------------------------------------------------

rest_service!({
    service_name: NeedsProviderService,
    base_path: "/np",
    endpoints: [
        GET WITH_PERMISSIONS(["admin"]) secret() -> Value,
    ]
});

struct NeedsProviderImpl;

#[async_trait::async_trait]
impl NeedsProviderServiceTrait for NeedsProviderImpl {
    async fn get_secret(&self, _user: &AuthenticatedUser) -> RestResult<Value> {
        Ok(RestResponse::ok(json!({ "ok": true })))
    }
}

#[test]
#[should_panic(expected = "auth_provider")]
fn build_panics_when_permissioned_service_has_no_provider() {
    // No `.auth_provider(...)` — must panic at build() rather than 500 at runtime.
    let _ = NeedsProviderServiceBuilder::new(NeedsProviderImpl).build();
}

// ---------------------------------------------------------------------------
// docs_require_auth gate
// ---------------------------------------------------------------------------

rest_service!({
    service_name: GatedDocsService,
    base_path: "/gd",
    openapi: true,
    serve_docs: true,
    docs_path: "/docs",
    docs_require_auth: true,
    endpoints: [
        GET WITH_PERMISSIONS(["admin"]) secret() -> Value,
    ]
});

struct GatedDocsImpl;

#[async_trait::async_trait]
impl GatedDocsServiceTrait for GatedDocsImpl {
    async fn get_secret(&self, _user: &AuthenticatedUser) -> RestResult<Value> {
        Ok(RestResponse::ok(json!({ "ok": true })))
    }
}

#[tokio::test]
async fn gated_docs_require_authentication() {
    let app = GatedDocsServiceBuilder::new(GatedDocsImpl)
        .auth_provider(MockAuth)
        .build();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    // Unauthenticated docs + openapi → rejected.
    assert_eq!(server.get("/gd/docs").await.status_code().as_u16(), 401);
    assert_eq!(
        server
            .get("/gd/docs/openapi.json")
            .await
            .status_code()
            .as_u16(),
        401
    );

    // With a valid credential → served.
    let docs = server
        .get("/gd/docs")
        .authorization_bearer("admin-token")
        .await;
    assert_eq!(docs.status_code().as_u16(), 200);
    let spec = server
        .get("/gd/docs/openapi.json")
        .authorization_bearer("admin-token")
        .await;
    assert_eq!(spec.status_code().as_u16(), 200);
}
