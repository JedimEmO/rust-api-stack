//! Regression tests for the `jsonrpc_service!` hardening that brings it to parity
//! with the `rest_service!` changes prompted by the XM device-integration feedback:
//!
//! * Content-Type enforcement (strict `application/json`, opt-out).
//! * Service-level `body_limit`.
//! * `docs_require_auth` gate on the explorer / openrpc routes.
//! * `build()` fails when a permissioned service has no auth provider.

use axum_test::TestServer;
use ras_jsonrpc_core::{AuthError, AuthProvider, AuthenticatedUser};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::pin::Pin;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PingRequest {
    value: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PingResponse {
    echo: String,
}

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

ras_jsonrpc_macro::jsonrpc_service!({
    service_name: StrictRpc,
    methods: [
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
    ]
});

struct StrictRpcImpl;

impl StrictRpcTrait for StrictRpcImpl {
    async fn ping(
        &self,
        request: PingRequest,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            echo: request.value,
        })
    }
}

ras_jsonrpc_macro::jsonrpc_service!({
    service_name: LenientRpc,
    require_json_content_type: false,
    methods: [
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
    ]
});

struct LenientRpcImpl;

impl LenientRpcTrait for LenientRpcImpl {
    async fn ping(
        &self,
        request: PingRequest,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            echo: request.value,
        })
    }
}

fn rpc_envelope() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "params": { "value": "hi" },
        "id": 1,
    })
}

#[tokio::test]
async fn strict_rpc_requires_json_content_type() {
    let router = StrictRpcBuilder::new(StrictRpcImpl).build().unwrap();
    let server = TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();

    // application/json → accepted.
    let ok = server.post("/rpc").json(&rpc_envelope()).await;
    assert_eq!(ok.status_code().as_u16(), 200);

    // text/plain carrying a valid JSON-RPC envelope → 415, before dispatch.
    let rejected = server
        .post("/rpc")
        .text(rpc_envelope().to_string())
        .content_type("text/plain")
        .await;
    assert_eq!(rejected.status_code().as_u16(), 415);
}

#[tokio::test]
async fn lenient_rpc_accepts_any_content_type() {
    let router = LenientRpcBuilder::new(LenientRpcImpl).build().unwrap();
    let server = TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();

    let ok = server
        .post("/rpc")
        .text(rpc_envelope().to_string())
        .content_type("text/plain")
        .await;
    assert_eq!(ok.status_code().as_u16(), 200);
}

// ---------------------------------------------------------------------------
// Service-level body_limit
// ---------------------------------------------------------------------------

ras_jsonrpc_macro::jsonrpc_service!({
    service_name: TinyRpc,
    body_limit: 64,
    methods: [
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
    ]
});

struct TinyRpcImpl;

impl TinyRpcTrait for TinyRpcImpl {
    async fn ping(
        &self,
        request: PingRequest,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            echo: request.value,
        })
    }
}

#[tokio::test]
async fn rpc_body_limit_rejects_oversized_body() {
    let router = TinyRpcBuilder::new(TinyRpcImpl).build().unwrap();
    let server = TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();

    let big = json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "params": { "value": "x".repeat(256) },
        "id": 1,
    });
    let response = server.post("/rpc").json(&big).await;
    assert_eq!(response.status_code().as_u16(), 413);
}

// ---------------------------------------------------------------------------
// build() fails when a permissioned service has no auth provider
// ---------------------------------------------------------------------------

ras_jsonrpc_macro::jsonrpc_service!({
    service_name: NeedsProviderRpc,
    methods: [
        WITH_PERMISSIONS(["admin"]) secret(PingRequest) -> PingResponse,
    ]
});

struct NeedsProviderRpcImpl;

impl NeedsProviderRpcTrait for NeedsProviderRpcImpl {
    async fn secret(
        &self,
        _user: &AuthenticatedUser,
        request: PingRequest,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            echo: request.value,
        })
    }
}

#[test]
fn build_errors_when_permissioned_service_has_no_provider() {
    let result = NeedsProviderRpcBuilder::new(NeedsProviderRpcImpl).build();
    let err = result.expect_err("build should fail without an auth provider");
    assert!(
        err.contains("auth_provider"),
        "error should mention auth_provider, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// docs_require_auth gate on the explorer / openrpc routes
// ---------------------------------------------------------------------------

ras_jsonrpc_macro::jsonrpc_service!({
    service_name: GatedDocsRpc,
    openrpc: true,
    explorer: true,
    docs_require_auth: true,
    methods: [
        UNAUTHORIZED ping(PingRequest) -> PingResponse,
        WITH_PERMISSIONS(["admin"]) secret(PingRequest) -> PingResponse,
    ]
});

struct GatedDocsRpcImpl;

impl GatedDocsRpcTrait for GatedDocsRpcImpl {
    async fn ping(
        &self,
        request: PingRequest,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            echo: request.value,
        })
    }

    async fn secret(
        &self,
        _user: &AuthenticatedUser,
        request: PingRequest,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(PingResponse {
            echo: request.value,
        })
    }
}

#[tokio::test]
async fn gated_explorer_requires_authentication() {
    let router = GatedDocsRpcBuilder::new(GatedDocsRpcImpl)
        .auth_provider(MockAuth)
        .build()
        .unwrap();
    let server = TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();

    // The explorer is served relative to the RPC base path ("/rpc").
    // Unauthenticated explorer + openrpc → rejected.
    assert_eq!(
        server.get("/rpc/explorer").await.status_code().as_u16(),
        401
    );
    assert_eq!(
        server
            .get("/rpc/explorer/openrpc.json")
            .await
            .status_code()
            .as_u16(),
        401
    );

    // With a valid credential → served.
    let explorer = server
        .get("/rpc/explorer")
        .authorization_bearer("admin-token")
        .await;
    assert_eq!(explorer.status_code().as_u16(), 200);
    let spec = server
        .get("/rpc/explorer/openrpc.json")
        .authorization_bearer("admin-token")
        .await;
    assert_eq!(spec.status_code().as_u16(), 200);

    // The docs gate must NOT leak onto the RPC endpoint: an unauthenticated
    // UNAUTHORIZED method call still succeeds.
    let ping = server
        .post("/rpc")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "ping",
            "params": { "value": "hi" },
            "id": 1,
        }))
        .await;
    assert_eq!(ping.status_code().as_u16(), 200);
    let body: Value = ping.json();
    assert_eq!(body["result"]["echo"], "hi");
}
