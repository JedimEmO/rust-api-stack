//! The usage and duration hooks see every call that passes authorization,
//! and the duration hook reports whether the call succeeded.

use axum_test::TestServer;
use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Input {
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    value: String,
}

jsonrpc_service!({
    service_name: HookService,
    methods: [
        UNAUTHORIZED succeed(Input) -> Output,
        UNAUTHORIZED fail(Input) -> Output,
        WITH_PERMISSIONS(["admin"]) guarded(Input) -> Output,
    ]
});

struct HookServiceImpl;

impl HookServiceTrait for HookServiceImpl {
    async fn succeed(
        &self,
        request: Input,
    ) -> Result<Output, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Output {
            value: request.value,
        })
    }

    async fn fail(
        &self,
        _request: Input,
    ) -> Result<Output, Box<dyn std::error::Error + Send + Sync>> {
        Err(std::io::Error::other("handler failed").into())
    }

    async fn guarded(
        &self,
        _user: &AuthenticatedUser,
        request: Input,
    ) -> Result<Output, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Output {
            value: request.value,
        })
    }
}

struct TokenProvider;

impl AuthProvider for TokenProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let permissions: HashSet<String> = match token.as_str() {
                "admin-token" => ["admin".to_string()].into_iter().collect(),
                "user-token" => HashSet::new(),
                _ => return Err(AuthError::InvalidToken),
            };
            Ok(AuthenticatedUser {
                user_id: token,
                permissions,
                metadata: None,
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Completed {
    method: String,
    user: Option<String>,
    success: bool,
}

#[derive(Clone, Default)]
struct Recorded {
    started: Arc<Mutex<Vec<String>>>,
    completed: Arc<Mutex<Vec<Completed>>>,
}

impl Recorded {
    fn started(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }

    fn completed(&self) -> Vec<Completed> {
        self.completed.lock().unwrap().clone()
    }
}

fn server() -> (TestServer, Recorded) {
    let recorded = Recorded::default();
    let started = recorded.started.clone();
    let completed = recorded.completed.clone();

    let router = HookServiceBuilder::new(HookServiceImpl)
        .auth_provider(TokenProvider)
        .with_usage_tracker(move |_headers, _user, request| {
            started.lock().unwrap().push(request.method.clone());
            async {}
        })
        .with_method_duration_tracker(move |method, user, _duration, success| {
            completed.lock().unwrap().push(Completed {
                method: method.to_string(),
                user: user.map(|user| user.user_id.clone()),
                success,
            });
            async {}
        })
        .build()
        .expect("router builds");

    let server = TestServer::builder()
        .mock_transport()
        .build(router)
        .expect("test server");
    (server, recorded)
}

async fn call(server: &TestServer, method: &str, params: Value, token: Option<&str>) -> Value {
    let mut request = server
        .post("/rpc")
        .json(&json!({ "jsonrpc": "2.0", "method": method, "params": params, "id": 1 }));
    if let Some(token) = token {
        request = request.authorization_bearer(token);
    }
    request.await.json()
}

fn completed(method: &str, user: Option<&str>, success: bool) -> Completed {
    Completed {
        method: method.to_string(),
        user: user.map(str::to_string),
        success,
    }
}

#[tokio::test]
async fn a_successful_call_is_recorded_as_a_success() {
    let (server, recorded) = server();

    let body = call(&server, "succeed", json!({ "value": "hi" }), None).await;

    assert_eq!(body["result"]["value"], "hi");
    assert_eq!(recorded.started(), vec!["succeed"]);
    assert_eq!(recorded.completed(), vec![completed("succeed", None, true)]);
}

#[tokio::test]
async fn a_handler_error_is_recorded_as_a_failure() {
    let (server, recorded) = server();

    let body = call(&server, "fail", json!({ "value": "hi" }), None).await;

    assert!(body["error"].is_object(), "{body}");
    assert_eq!(recorded.started(), vec!["fail"]);
    assert_eq!(recorded.completed(), vec![completed("fail", None, false)]);
}

#[tokio::test]
async fn invalid_params_are_recorded_as_a_failure() {
    let (server, recorded) = server();

    let body = call(&server, "succeed", json!({ "wrong": 1 }), None).await;

    assert_eq!(body["error"]["code"], -32602, "{body}");
    assert_eq!(recorded.started(), vec!["succeed"]);
    assert_eq!(
        recorded.completed(),
        vec![completed("succeed", None, false)]
    );
}

#[tokio::test]
async fn an_authorized_call_reports_its_user() {
    let (server, recorded) = server();

    let body = call(
        &server,
        "guarded",
        json!({ "value": "hi" }),
        Some("admin-token"),
    )
    .await;

    assert_eq!(body["result"]["value"], "hi");
    assert_eq!(
        recorded.completed(),
        vec![completed("guarded", Some("admin-token"), true)]
    );
}

#[tokio::test]
async fn rejected_and_unknown_calls_reach_neither_hook() {
    let (server, recorded) = server();

    // Missing credential, missing permission, and a method the service does not declare.
    call(&server, "guarded", json!({ "value": "hi" }), None).await;
    call(
        &server,
        "guarded",
        json!({ "value": "hi" }),
        Some("user-token"),
    )
    .await;
    call(&server, "nope", json!({}), None).await;

    assert!(recorded.started().is_empty(), "{:?}", recorded.started());
    assert!(
        recorded.completed().is_empty(),
        "{:?}",
        recorded.completed()
    );
}
