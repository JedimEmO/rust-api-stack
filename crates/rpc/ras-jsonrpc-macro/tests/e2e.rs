//! End-to-end test that exercises the full in-memory chain:
//!   axum-test request -> axum router -> handler -> response.
//!
//! Covers: success path, missing-permission rejection, malformed input.

use ras_jsonrpc_macro::jsonrpc_service;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod support;
use support::{MockAuthProvider, mock_http_server};
#[cfg(feature = "client")]
use support::{axum_transport, mock_http_server_arc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EchoRequest {
    msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EchoResponse {
    msg: String,
    user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddRequest {
    a: i64,
    b: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddResponse {
    sum: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenameUserV1 {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenameUserV2 {
    display_name: String,
    notify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenameUserResponseV1 {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenameUserResponseV2 {
    display_name: String,
    notified: bool,
}

struct RenameUserCompat;

impl ras_jsonrpc_core::VersionMigration<RenameUserV1, RenameUserV2> for RenameUserCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserV1) -> Result<RenameUserV2, Self::Error> {
        Ok(RenameUserV2 {
            display_name: value.name,
            notify: false,
        })
    }
}

impl ras_jsonrpc_core::VersionMigration<RenameUserResponseV2, RenameUserResponseV1>
    for RenameUserCompat
{
    type Error = std::convert::Infallible;

    fn migrate(value: RenameUserResponseV2) -> Result<RenameUserResponseV1, Self::Error> {
        Ok(RenameUserResponseV1 {
            name: value.display_name,
        })
    }
}

jsonrpc_service!({
    service_name: Demo,
    openrpc: false,
    methods: [
        UNAUTHORIZED ping(EchoRequest) -> EchoResponse,
        UNAUTHORIZED rename_user(RenameUserV2) -> RenameUserResponseV2 {
            version: "2.0.0",
            wire: "rename_user.v2",
            versions: [
                "1.0.0" {
                    wire: "rename_user.v1",
                    request: RenameUserV1,
                    response: RenameUserResponseV1,
                    migration: RenameUserCompat,
                },
            ],
        },
        WITH_PERMISSIONS(["user"]) add(AddRequest) -> AddResponse,
        WITH_PERMISSIONS(["admin"]) admin_only(EchoRequest) -> EchoResponse,
    ]
});

struct DemoImpl;

impl DemoTrait for DemoImpl {
    async fn ping(
        &self,
        req: EchoRequest,
    ) -> Result<EchoResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(EchoResponse {
            msg: req.msg,
            user_id: None,
        })
    }

    async fn add(
        &self,
        _user: &ras_jsonrpc_core::AuthenticatedUser,
        req: AddRequest,
    ) -> Result<AddResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(AddResponse { sum: req.a + req.b })
    }

    async fn rename_user(
        &self,
        req: RenameUserV2,
    ) -> Result<RenameUserResponseV2, Box<dyn std::error::Error + Send + Sync>> {
        Ok(RenameUserResponseV2 {
            display_name: req.display_name,
            notified: req.notify,
        })
    }

    async fn admin_only(
        &self,
        user: &ras_jsonrpc_core::AuthenticatedUser,
        req: EchoRequest,
    ) -> Result<EchoResponse, Box<dyn std::error::Error + Send + Sync>> {
        Ok(EchoResponse {
            msg: req.msg,
            user_id: Some(user.user_id.clone()),
        })
    }
}

fn router() -> axum::Router {
    DemoBuilder::new(DemoImpl)
        .base_url("/rpc")
        .auth_provider(MockAuthProvider::default())
        .build()
        .expect("build router")
}

fn server() -> axum_test::TestServer {
    mock_http_server(router())
}

#[cfg(feature = "client")]
#[test]
fn versioned_client_method_names_sanitize_semver_labels() {
    let _method = DemoClient::rename_user_v1_0_0;
    let _method_with_timeout = DemoClient::rename_user_v1_0_0_with_timeout;
}

/// Build the generated `DemoClient` wired to drive requests through the
/// in-process `AxumTestTransport`, exercising the full envelope-build +
/// transport-execute + error-extraction path of the migrated client.
#[cfg(feature = "client")]
fn demo_client() -> DemoClient {
    let server = mock_http_server_arc(router());
    let transport = axum_transport(server);
    DemoClientBuilder::new()
        // The AxumTestTransport strips scheme+authority, so the host is
        // irrelevant; only the path "/rpc" matters.
        .server_url("http://in-memory.test/rpc")
        .build_with_transport(transport)
        .expect("build DemoClient over AxumTestTransport")
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_round_trips_over_axum_transport() {
    let client = demo_client();

    let resp = client
        .ping(EchoRequest {
            msg: "hello-from-client".to_string(),
        })
        .await
        .expect("ping over transport should succeed");

    assert_eq!(resp.msg, "hello-from-client");
    assert_eq!(resp.user_id, None);
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_sends_bearer_and_succeeds_with_permission() {
    let mut client = demo_client();
    client.set_bearer_token(Some("user-token"));

    let resp = client
        .add(AddRequest { a: 7, b: 35 })
        .await
        .expect("authenticated add should succeed");

    assert_eq!(resp.sum, 42);
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_surfaces_jsonrpc_error_on_missing_permission() {
    let client = demo_client();

    let err = client
        .add(AddRequest { a: 1, b: 2 })
        .await
        .expect_err("anonymous add must be rejected as a JSON-RPC error");

    match err {
        ras_transport_core::TransportError::JsonRpc { message, .. } => {
            let m = message.to_lowercase();
            assert!(
                m.contains("auth") || m.contains("permission"),
                "expected auth/permission error, got: {message}"
            );
        }
        other => panic!("expected JsonRpc error variant, got: {other:?}"),
    }
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_round_trips_versioned_wire_method() {
    let client = demo_client();

    let resp = client
        .rename_user_v1_0_0(RenameUserV1 {
            name: "Ada".to_string(),
        })
        .await
        .expect("legacy versioned method should round-trip via client");

    assert_eq!(
        resp,
        RenameUserResponseV1 {
            name: "Ada".to_string()
        }
    );
}

async fn call_rpc<T>(
    server: &axum_test::TestServer,
    method: &str,
    params: Value,
    token: Option<&str>,
) -> Result<T, Value>
where
    T: DeserializeOwned,
{
    let body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1,
    });

    let mut request = server.post("/rpc").json(&body);
    if let Some(token) = token {
        request = request.authorization_bearer(token);
    }

    let payload: Value = request.await.json();

    if let Some(error) = payload.get("error") {
        Err(error.clone())
    } else {
        Ok(serde_json::from_value(payload["result"].clone()).expect("result should deserialize"))
    }
}

#[tokio::test]
async fn legacy_version_round_trips_through_canonical_handler() {
    let server = server();

    let resp: RenameUserResponseV1 = call_rpc(
        &server,
        "rename_user.v1",
        json!(RenameUserV1 {
            name: "Ada".to_string(),
        }),
        None,
    )
    .await
    .expect("legacy rename ok");

    assert_eq!(
        resp,
        RenameUserResponseV1 {
            name: "Ada".to_string()
        }
    );
}

#[tokio::test]
async fn canonical_version_uses_declared_wire_method() {
    let server = server();

    let resp: RenameUserResponseV2 = call_rpc(
        &server,
        "rename_user.v2",
        json!(RenameUserV2 {
            display_name: "Grace".to_string(),
            notify: true,
        }),
        None,
    )
    .await
    .expect("canonical rename ok");

    assert_eq!(
        resp,
        RenameUserResponseV2 {
            display_name: "Grace".to_string(),
            notified: true,
        }
    );
}

#[tokio::test]
async fn unauth_method_round_trips() {
    let server = server();

    let resp: EchoResponse = call_rpc(
        &server,
        "ping",
        json!(EchoRequest {
            msg: "hello".to_string(),
        }),
        None,
    )
    .await
    .expect("ping ok");

    assert_eq!(resp.msg, "hello");
    assert_eq!(resp.user_id, None);
}

#[tokio::test]
async fn permission_required_method_rejects_anonymous() {
    let server = server();

    let err = call_rpc::<AddResponse>(&server, "add", json!(AddRequest { a: 2, b: 3 }), None)
        .await
        .expect_err("anonymous add must be rejected");

    let s = err.to_string();
    assert!(
        s.contains("Authentication") || s.contains("AUTH") || s.contains("auth"),
        "expected auth-related error, got: {s}"
    );
}

#[tokio::test]
async fn permission_required_method_rejects_wrong_perms() {
    let server = server();

    let err = call_rpc::<AddResponse>(
        &server,
        "add",
        json!(AddRequest { a: 2, b: 3 }),
        Some("readonly-token"),
    )
    .await
    .expect_err("readonly user must not be allowed to call add");
    let s = err.to_string();
    assert!(
        s.contains("permission") || s.contains("Permission") || s.contains("PERMISSION"),
        "expected permission-related error, got: {s}"
    );
}

#[tokio::test]
async fn permission_required_method_succeeds_with_correct_perms() {
    let server = server();

    let resp: AddResponse = call_rpc(
        &server,
        "add",
        json!(AddRequest { a: 7, b: 35 }),
        Some("user-token"),
    )
    .await
    .expect("add ok");
    assert_eq!(resp.sum, 42);
}

#[tokio::test]
async fn admin_method_succeeds_with_admin_token() {
    let server = server();

    let resp: EchoResponse = call_rpc(
        &server,
        "admin_only",
        json!(EchoRequest {
            msg: "secret".to_string(),
        }),
        Some("admin-token"),
    )
    .await
    .expect("admin call ok");

    assert_eq!(resp.msg, "secret");
    assert_eq!(resp.user_id.as_deref(), Some("admin-1"));
}

#[tokio::test]
async fn malformed_params_yield_jsonrpc_error() {
    // Bypass the typed client to send a malformed body and confirm the
    // server returns a JSON-RPC `invalid_params` error rather than a panic.
    let server = server();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "params": { "bogus": 1 },
        "id": 1,
    });

    let resp: serde_json::Value = server.post("/rpc").json(&body).await.json();

    assert!(
        resp.get("error").is_some(),
        "expected error in response: {resp}"
    );
    let code = resp["error"]["code"].as_i64().unwrap();
    assert_eq!(code, -32602, "expected invalid_params (-32602), got {code}");
}
