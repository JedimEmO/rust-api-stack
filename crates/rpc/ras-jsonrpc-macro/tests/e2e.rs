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
        OPTIONAL_AUTH whoami(EchoRequest) -> EchoResponse,
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

    async fn whoami(
        &self,
        caller: ras_jsonrpc_core::Caller,
        req: EchoRequest,
    ) -> Result<EchoResponse, Box<dyn std::error::Error + Send + Sync>> {
        // OPTIONAL_AUTH: report the caller when present, anonymous otherwise.
        Ok(EchoResponse {
            msg: req.msg,
            user_id: caller.authenticated().map(|user| user.user_id.clone()),
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

/// Build the generated `DemoClient` wired to drive requests through the
/// in-process `AxumTestTransport`, exercising the full envelope-build +
/// transport-execute + error-extraction path of the migrated client.
#[cfg(feature = "client")]
fn demo_client() -> DemoClient {
    let server = mock_http_server_arc(router());
    let transport = axum_transport(server);
    DemoClientBuilder::new("http://in-memory.test/rpc")
        // The AxumTestTransport strips scheme+authority, so the host is
        // irrelevant; only the path "/rpc" matters.
        .build_with_transport(transport)
        .expect("build DemoClient over AxumTestTransport")
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

#[path = "e2e/auth.rs"]
mod auth;
#[path = "e2e/client.rs"]
mod client;
#[path = "e2e/parameters.rs"]
mod parameters;
#[path = "e2e/versioning.rs"]
mod versioning;
