//! A request without an `id` is a notification: the server runs it and sends no
//! JSON-RPC response. `"id": null` is an ordinary request.

use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use serde_json::{Value, json};
use std::sync::Mutex;

jsonrpc_service!({
    service_name: NotifyService,
    methods: [
        UNAUTHORIZED record(String) -> u32,
        UNAUTHORIZED fail(()) -> (),
        WITH_PERMISSIONS(["admin"]) admin(()) -> (),
    ]
});

static RECORDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Notify;

impl NotifyServiceTrait for Notify {
    async fn record(
        &self,
        request: String,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let mut recorded = RECORDED.lock().unwrap();
        recorded.push(request);
        Ok(recorded.len() as u32)
    }
    async fn fail(&self, _request: ()) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err(ras_jsonrpc_types::PublicRpcError::new(1001, "nope").into())
    }
    async fn admin(
        &self,
        _user: &AuthenticatedUser,
        _request: (),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

struct RejectAll;

impl AuthProvider for RejectAll {
    fn authenticate(&self, _token: String) -> AuthFuture<'_> {
        Box::pin(async { Err(AuthError::InvalidToken) })
    }
}

fn server() -> axum_test::TestServer {
    let router = NotifyServiceBuilder::new(Notify)
        .base_url("/rpc")
        .auth_provider(RejectAll)
        .build()
        .unwrap();
    axum_test::TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap()
}

async fn post(server: &axum_test::TestServer, body: Value) -> (u16, String) {
    let response = server.post("/rpc").json(&body).await;
    (response.status_code().as_u16(), response.text())
}

#[tokio::test]
async fn a_notification_runs_and_gets_no_response() {
    let (status, body) = post(
        &server(),
        json!({ "jsonrpc": "2.0", "method": "record", "params": "from a notification" }),
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(body, "");
    assert!(
        RECORDED
            .lock()
            .unwrap()
            .iter()
            .any(|r| r == "from a notification"),
        "the handler ran"
    );
}

#[tokio::test]
async fn a_failing_notification_gets_no_response_either() {
    let (status, body) = post(&server(), json!({ "jsonrpc": "2.0", "method": "fail" })).await;
    assert_eq!(status, 204);
    assert_eq!(body, "");

    let (status, body) = post(&server(), json!({ "jsonrpc": "2.0", "method": "missing" })).await;
    assert_eq!(status, 204, "an unknown method is not answered either");
    assert_eq!(body, "");
}

#[tokio::test]
async fn an_unauthorized_notification_keeps_its_http_status() {
    let (status, body) = post(&server(), json!({ "jsonrpc": "2.0", "method": "admin" })).await;
    assert_eq!(status, 401);
    assert_eq!(body, "");
}

#[tokio::test]
async fn a_null_id_is_a_request_answered_with_a_null_id() {
    let (status, body) = post(
        &server(),
        json!({ "jsonrpc": "2.0", "method": "record", "params": "x", "id": null }),
    )
    .await;
    assert_eq!(status, 200);
    let response: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(response.get("id"), Some(&Value::Null));
    assert!(response["result"].is_number(), "{response}");
}

#[tokio::test]
async fn unreadable_requests_are_answered_with_a_null_id() {
    let server = server();
    let response = server
        .post("/rpc")
        .text("{not json")
        .content_type("application/json")
        .await;
    let body: Value = response.json();
    assert_eq!(body["error"]["code"], -32700);
    assert_eq!(body.get("id"), Some(&Value::Null));

    let (status, text) = post(&server, json!({ "jsonrpc": "1.0", "method": "record" })).await;
    assert_eq!(
        status, 200,
        "a wrong version is never treated as a notification"
    );
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["code"], -32600);
    assert_eq!(body.get("id"), Some(&Value::Null));
}
