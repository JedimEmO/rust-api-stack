//! A handler's error reaches the client only when it returns a
//! `PublicRpcError` directly, with a code it is allowed to use.

use ras_jsonrpc_macro::jsonrpc_service;
use ras_jsonrpc_types::{JsonRpcError, PublicRpcError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Fail {
    kind: String,
    #[serde(default)]
    code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Done {
    ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FailV1 {
    kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DoneV1 {
    ok: bool,
}

#[derive(Debug)]
pub struct MigrationError(&'static str);

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for MigrationError {}

pub struct FailCompat;

impl ras_jsonrpc_core::VersionMigration<FailV1, Fail> for FailCompat {
    type Error = MigrationError;

    fn migrate(value: FailV1) -> Result<Fail, Self::Error> {
        if value.kind == "reject" {
            return Err(MigrationError("secret request migration detail"));
        }
        Ok(Fail {
            kind: value.kind,
            code: 1001,
        })
    }
}

impl ras_jsonrpc_core::VersionMigration<Done, DoneV1> for FailCompat {
    type Error = MigrationError;

    fn migrate(value: Done) -> Result<DoneV1, Self::Error> {
        if !value.ok {
            return Err(MigrationError("secret response migration detail"));
        }
        Ok(DoneV1 { ok: value.ok })
    }
}

jsonrpc_service!({
    service_name: ErrorService,
    openrpc: true,
    methods: [
        UNAUTHORIZED fail(Fail) -> Done {
            version: v2,
            wire: "fail.v2",
            versions: [
                v1 {
                    wire: "fail.v1",
                    request: FailV1,
                    response: DoneV1,
                    migration: FailCompat,
                },
            ],
        },
    ]
});

/// An ordinary error that happens to carry JSON-RPC detail, like an upstream client error.
#[derive(Debug)]
struct CarriesWireError(JsonRpcError);

impl fmt::Display for CarriesWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "upstream: {}", self.0.message)
    }
}

impl std::error::Error for CarriesWireError {}

#[derive(Debug, thiserror::Error)]
enum Wrapped {
    #[error(transparent)]
    Public(#[from] PublicRpcError),
}

struct Failing;

impl ErrorServiceTrait for Failing {
    async fn fail(&self, request: Fail) -> Result<Done, Box<dyn std::error::Error + Send + Sync>> {
        let secret = || PublicRpcError::new(1001, "secret detail");
        match request.kind.as_str() {
            "public" => Err(
                PublicRpcError::new(request.code, "Email already registered")
                    .with_data(json!({ "field": "email" }))
                    .into(),
            ),
            "plain" => Err(std::io::Error::other("password=hunter2").into()),
            "carries_wire" => Err(Box::new(CarriesWireError(JsonRpcError::new(
                1001,
                "secret detail".into(),
                None,
            )))),
            "thiserror" => Err(Wrapped::from(secret()).into()),
            "anyhow" => Err(anyhow::Error::new(secret()).into()),
            "anyhow_context" => Err(anyhow::Error::new(secret()).context("while saving").into()),
            "break_response" => Ok(Done { ok: false }),
            _ => Ok(Done { ok: true }),
        }
    }
}

fn router() -> axum::Router {
    ErrorServiceBuilder::new(Failing)
        .base_url("/rpc")
        .build()
        .expect("router builds")
}

fn server() -> axum_test::TestServer {
    axum_test::TestServer::builder()
        .mock_transport()
        .build(router())
        .expect("test server")
}

/// Returns the HTTP status and the JSON-RPC response body.
async fn call(server: &axum_test::TestServer, method: &str, params: Value) -> (u16, Value) {
    let response = server
        .post("/rpc")
        .json(&json!({ "jsonrpc": "2.0", "method": method, "params": params, "id": 1 }))
        .await;
    (response.status_code().as_u16(), response.json())
}

fn assert_hidden(label: &str, body: &Value) {
    assert_eq!(body["error"]["code"], -32603, "{label}: {body}");
    assert_eq!(
        body["error"]["message"], "Internal error",
        "{label}: {body}"
    );
    assert!(body["error"].get("data").is_none(), "{label}: {body}");
    let text = body.to_string();
    assert!(!text.contains("secret"), "{label} leaked: {text}");
    assert!(!text.contains("hunter2"), "{label} leaked: {text}");
}

#[tokio::test]
async fn a_returned_public_error_is_sent_with_http_200() {
    let server = server();
    for method in ["fail.v2", "fail.v1"] {
        let (status, body) = call(&server, method, json!({ "kind": "public", "code": 1001 })).await;
        assert_eq!(status, 200, "{method}");
        assert_eq!(
            body["error"],
            json!({ "code": 1001, "message": "Email already registered", "data": { "field": "email" } }),
            "{method}"
        );
    }
}

#[tokio::test]
async fn allowed_and_reserved_codes() {
    let server = server();
    for code in [-32769, -31999, -32602, 0, 1001] {
        let (status, body) = call(
            &server,
            "fail.v2",
            json!({ "kind": "public", "code": code }),
        )
        .await;
        assert_eq!(status, 200, "{code}");
        assert_eq!(body["error"]["code"], code, "{code}: {body}");
    }
    for code in [-32768, -32000, -32001, -32002, -32003, -32004, -32603] {
        let (status, body) = call(
            &server,
            "fail.v2",
            json!({ "kind": "public", "code": code }),
        )
        .await;
        // Never the 401/403 the HTTP layer gives the auth codes.
        assert_eq!(status, 200, "{code}");
        assert_hidden(&format!("reserved {code}"), &body);
    }
}

#[tokio::test]
async fn other_errors_stay_hidden() {
    let server = server();
    // `anyhow::Error` converts into `Box<dyn Error>` as its own wrapper type, so
    // a `PublicRpcError` passed through anyhow is hidden too, context or not.
    for kind in [
        "plain",
        "carries_wire",
        "thiserror",
        "anyhow",
        "anyhow_context",
    ] {
        for method in ["fail.v2", "fail.v1"] {
            let (_, body) = call(&server, method, json!({ "kind": kind })).await;
            assert_hidden(&format!("{method} {kind}"), &body);
        }
    }
}

#[tokio::test]
async fn migration_failures_keep_their_codes() {
    let server = server();
    let (_, rejected) = call(&server, "fail.v1", json!({ "kind": "reject" })).await;
    assert_eq!(rejected["error"]["code"], -32602, "{rejected}");
    assert_eq!(rejected["error"]["message"], "Invalid params", "{rejected}");

    let (_, broken) = call(&server, "fail.v1", json!({ "kind": "break_response" })).await;
    assert_hidden("response migration", &broken);
}

#[cfg(feature = "client")]
#[tokio::test]
async fn the_generated_client_exposes_code_message_and_data() {
    let server = std::sync::Arc::new(server());
    let transport: std::sync::Arc<dyn ras_transport_core::HttpTransport> =
        std::sync::Arc::new(ras_transport_core::AxumTestTransport::from_arc(server));
    let client = ErrorServiceClientBuilder::new("http://in-memory.test/rpc")
        .build_with_transport(transport)
        .expect("client builds");

    let error = client
        .fail(Fail {
            kind: "public".into(),
            code: 1001,
        })
        .await
        .expect_err("handler fails");
    match error {
        ras_transport_core::TransportError::JsonRpc {
            code,
            message,
            data,
        } => {
            assert_eq!(code, 1001);
            assert_eq!(message, "Email already registered");
            assert_eq!(data, Some(json!({ "field": "email" })));
        }
        other => panic!("expected a JSON-RPC error, got {other:?}"),
    }
}
