//! The usage and duration hooks see every request that passes authorization,
//! and the duration hook reports whether the request succeeded.

use axum_test::TestServer;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_rest_core::{RestError, RestResponse, RestResult};
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Thing {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThingV1 {
    title: String,
}

rest_service!({
    service_name: Hooks,
    base_path: "/api",
    endpoints: [
        GET UNAUTHORIZED ok() -> Thing,
        GET UNAUTHORIZED broken() -> Thing,
        POST UNAUTHORIZED echo(Thing) -> Thing,
        GET WITH_PERMISSIONS(["admin"]) guarded() -> Thing,
        POST UNAUTHORIZED v2/things(Thing) -> Thing {
            version: v2,
            versions: [
                v1 {
                    path: v1/things,
                    body: ThingV1,
                    response: ThingV1,
                    migration: ThingCompat,
                },
            ],
        },
    ]
});

#[derive(Debug)]
struct Unmigratable;

impl std::fmt::Display for Unmigratable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cannot migrate")
    }
}

impl std::error::Error for Unmigratable {}

struct ThingCompat;

impl ras_rest_core::VersionMigration<HooksPostV2ThingsV1Request, HooksPostV2ThingsV2Request>
    for ThingCompat
{
    type Error = std::convert::Infallible;

    fn migrate(
        value: HooksPostV2ThingsV1Request,
    ) -> Result<HooksPostV2ThingsV2Request, Self::Error> {
        Ok(HooksPostV2ThingsV2Request {
            path: HooksPostV2ThingsV2Path {},
            query: HooksPostV2ThingsV2Query {},
            body: Thing {
                name: value.body.title,
            },
        })
    }
}

impl ras_rest_core::VersionMigration<Thing, ThingV1> for ThingCompat {
    type Error = Unmigratable;

    fn migrate(value: Thing) -> Result<ThingV1, Self::Error> {
        if value.name == "unmigratable" {
            return Err(Unmigratable);
        }
        Ok(ThingV1 { title: value.name })
    }
}

struct HooksImpl;

#[async_trait::async_trait]
impl HooksTrait for HooksImpl {
    async fn get_ok(&self) -> RestResult<Thing> {
        Ok(RestResponse::ok(Thing {
            name: "ok".to_string(),
        }))
    }

    async fn get_broken(&self) -> RestResult<Thing> {
        Err(RestError::not_found("no such thing"))
    }

    async fn post_echo(&self, body: Thing) -> RestResult<Thing> {
        Ok(RestResponse::ok(body))
    }

    async fn get_guarded(&self, _user: &AuthenticatedUser) -> RestResult<Thing> {
        Ok(RestResponse::ok(Thing {
            name: "guarded".to_string(),
        }))
    }

    async fn post_v2_things(&self, body: Thing) -> RestResult<Thing> {
        Ok(RestResponse::ok(body))
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
    route: String,
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

    let router = HooksBuilder::new(HooksImpl)
        .auth_provider(TokenProvider)
        .with_usage_tracker(move |_headers, _user, method, path| {
            started.lock().unwrap().push(format!("{method} {path}"));
            async {}
        })
        .with_method_duration_tracker(move |method, path, user, _duration, success| {
            completed.lock().unwrap().push(Completed {
                route: format!("{method} {path}"),
                user: user.map(|user| user.user_id.clone()),
                success,
            });
            async {}
        })
        .build();

    let server = TestServer::builder()
        .mock_transport()
        .build(router)
        .expect("test server");
    (server, recorded)
}

fn completed(route: &str, user: Option<&str>, success: bool) -> Completed {
    Completed {
        route: route.to_string(),
        user: user.map(str::to_string),
        success,
    }
}

#[tokio::test]
async fn a_successful_request_is_recorded_as_a_success() {
    let (server, recorded) = server();

    server.get("/api/ok").await.assert_status_ok();

    assert_eq!(recorded.started(), vec!["GET /ok"]);
    assert_eq!(recorded.completed(), vec![completed("GET /ok", None, true)]);
}

#[tokio::test]
async fn a_handler_error_is_recorded_as_a_failure() {
    let (server, recorded) = server();

    server.get("/api/broken").await.assert_status_not_found();

    assert_eq!(recorded.started(), vec!["GET /broken"]);
    assert_eq!(
        recorded.completed(),
        vec![completed("GET /broken", None, false)]
    );
}

#[tokio::test]
async fn an_invalid_body_is_recorded_as_a_failure() {
    let (server, recorded) = server();

    server
        .post("/api/echo")
        .json(&json!({ "wrong": 1 }))
        .expect_failure()
        .await;

    assert_eq!(recorded.started(), vec!["POST /echo"]);
    assert_eq!(
        recorded.completed(),
        vec![completed("POST /echo", None, false)]
    );
}

#[tokio::test]
async fn a_failed_response_migration_is_recorded_as_a_failure() {
    let (server, recorded) = server();

    server
        .post("/api/v1/things")
        .json(&json!({ "title": "fine" }))
        .await
        .assert_status_ok();
    server
        .post("/api/v1/things")
        .json(&json!({ "title": "unmigratable" }))
        .expect_failure()
        .await;

    assert_eq!(
        recorded.completed(),
        vec![
            completed("POST /v1/things", None, true),
            completed("POST /v1/things", None, false),
        ]
    );
}

#[tokio::test]
async fn an_authorized_request_reports_its_user() {
    let (server, recorded) = server();

    server
        .get("/api/guarded")
        .authorization_bearer("admin-token")
        .await
        .assert_status_ok();

    assert_eq!(
        recorded.completed(),
        vec![completed("GET /guarded", Some("admin-token"), true)]
    );
}

#[tokio::test]
async fn rejected_requests_reach_neither_hook() {
    let (server, recorded) = server();

    server.get("/api/guarded").expect_failure().await;
    server
        .get("/api/guarded")
        .authorization_bearer("user-token")
        .expect_failure()
        .await;

    assert!(recorded.started().is_empty(), "{:?}", recorded.started());
    assert!(
        recorded.completed().is_empty(),
        "{:?}",
        recorded.completed()
    );
}
