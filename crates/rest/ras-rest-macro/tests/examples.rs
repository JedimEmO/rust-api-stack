//! Authored REST examples appear under one key in every place they touch,
//! match their schemas, and describe what the server actually answers.

use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_rest_core::{RestError, RestResponse, RestResult};
use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameItem {
    name: String,
    priority: u32,
    archived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    id: u32,
    name: String,
    nickname: Option<String>,
}

fn spare_parts(id: u32) -> Item {
    Item {
        id,
        name: "Spare parts".into(),
        nickname: None,
    }
}

fn rename_body() -> RenameItem {
    RenameItem {
        name: "Spare parts".into(),
        priority: 0,
        archived: false,
    }
}

rest_service!({
    service_name: ItemService,
    base_path: "/api",
    openapi: true,
    serve_docs: true,
    endpoints: [
        /// Rename an item.
        POST UNAUTHORIZED items/{id: u32}/rename ? notify: Option<bool> (RenameItem) -> Item {
            examples: [
                /// Rename and notify watchers.
                ///
                /// Zero and false values are kept as written.
                notify_watchers {
                    path: { id: 42 },
                    query: { notify: Some(true) },
                    body: rename_body(),
                    response: spare_parts(42),
                },
                /// Leave the optional query parameter out.
                quiet {
                    path: { id: 7 },
                    query: { notify: None },
                    body: rename_body(),
                    response: spare_parts(7),
                },
                /// The item does not exist.
                missing_item {
                    path: { id: 9999 },
                    body: rename_body(),
                    error: { status: 404, message: "Item not found" },
                },
                /// The name is empty.
                empty_name {
                    path: { id: 1 },
                    body: RenameItem { name: String::new(), priority: 0, archived: false },
                    error: { status: 400, message: "Name must not be empty" },
                },
            ],
        },
        POST UNAUTHORIZED items (RenameItem) -> Item {
            examples: [
                created { body: rename_body(), status: 201, response: spare_parts(5) },
            ],
        },
        DELETE UNAUTHORIZED items/{id: u32} () -> () {
            examples: [
                deleted { path: { id: 1 }, status: 204 },
                gone { path: { id: 2 }, error: { status: 410, message: "Already deleted" } },
            ],
        },
        GET UNAUTHORIZED search ? q: String & limit: Option<u32> () -> Vec<Item> {
            examples: [
                empty_query { query: { q: String::new() }, response: vec![] },
            ],
        },
    ]
});

struct Items;

#[async_trait::async_trait]
impl ItemServiceTrait for Items {
    async fn post_items_by_id_rename(
        &self,
        id: u32,
        _notify: Option<bool>,
        body: RenameItem,
    ) -> RestResult<Item> {
        if body.name.is_empty() {
            return Err(RestError::new(400, "Name must not be empty"));
        }
        if id == 9999 {
            return Err(RestError::not_found("Item not found"));
        }
        Ok(RestResponse::ok(Item {
            id,
            name: body.name,
            nickname: None,
        }))
    }

    async fn post_items(&self, body: RenameItem) -> RestResult<Item> {
        Ok(RestResponse::created(Item {
            id: 5,
            name: body.name,
            nickname: None,
        }))
    }

    async fn delete_items_by_id(&self, id: u32) -> RestResult<()> {
        if id == 2 {
            return Err(RestError::new(410, "Already deleted"));
        }
        Ok(RestResponse::with_status(204, ()))
    }

    async fn get_search(&self, _q: String, _limit: Option<u32>) -> RestResult<Vec<Item>> {
        Ok(RestResponse::ok(vec![]))
    }
}

fn doc() -> Value {
    try_generate_itemservice_openapi().expect("document builds")
}

fn rename_operation(doc: &Value) -> &Value {
    &doc["paths"]["/items/{id}/rename"]["post"]
}

fn parameter<'a>(operation: &'a Value, name: &str) -> &'a Value {
    operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap()
}

#[test]
fn document_with_examples_conforms() {
    ras_spec_conformance::assert_openapi3_conforms(&doc());
}

#[test]
fn an_example_appears_under_one_key_everywhere_it_touches() {
    let doc = doc();
    let operation = rename_operation(&doc);
    let id = &parameter(operation, "id")["examples"];
    let notify = &parameter(operation, "notify")["examples"];
    let body = &operation["requestBody"]["content"]["application/json"]["examples"];
    let ok = &operation["responses"]["200"]["content"]["application/json"]["examples"];

    assert_eq!(
        id["notify_watchers"],
        json!({
            "summary": "Rename and notify watchers.",
            "description": "Rename and notify watchers.\n\nZero and false values are kept as written.",
            "value": 42
        })
    );
    assert_eq!(notify["notify_watchers"]["value"], true);
    assert_eq!(
        body["notify_watchers"]["value"],
        json!({ "name": "Spare parts", "priority": 0, "archived": false })
    );
    assert_eq!(
        ok["notify_watchers"]["value"],
        json!({ "id": 42, "name": "Spare parts", "nickname": null })
    );

    // A query value left out, or `None`, is not listed.
    assert_eq!(id["quiet"]["value"], 7);
    assert!(notify.get("quiet").is_none());
}

#[test]
fn error_examples_use_the_error_body_at_their_status() {
    let doc = doc();
    assert_eq!(
        doc["components"]["schemas"]["RestErrorBody"]["required"],
        json!(["error"])
    );
    let operation = rename_operation(&doc);

    let not_found = &operation["responses"]["404"];
    assert_eq!(not_found["description"], "Error returned by the handler");
    let media = &not_found["content"]["application/json"];
    assert_eq!(
        media["schema"]["$ref"],
        "#/components/schemas/RestErrorBody"
    );
    assert_eq!(
        media["examples"]["missing_item"]["value"],
        json!({ "error": "Item not found" })
    );

    // 400 is also what the framework answers before the handler runs.
    let bad_request = &operation["responses"]["400"];
    assert!(
        bad_request["description"]
            .as_str()
            .unwrap()
            .contains("rejected before the handler runs"),
        "{bad_request}"
    );
    assert_eq!(
        bad_request["content"]["application/json"]["examples"]["empty_name"]["value"],
        json!({ "error": "Name must not be empty" })
    );
}

#[test]
fn success_statuses_get_their_own_responses() {
    let doc = doc();
    let created = &doc["paths"]["/items"]["post"]["responses"]["201"];
    assert_eq!(
        created["content"]["application/json"]["examples"]["created"]["value"]["id"],
        5
    );
    assert!(created["content"]["application/json"]["schema"].is_object());

    let delete = &doc["paths"]["/items/{id}"]["delete"]["responses"];
    assert_eq!(delete["204"], json!({ "description": "No content" }));
    assert_eq!(
        delete["410"]["content"]["application/json"]["examples"]["gone"]["value"],
        json!({ "error": "Already deleted" })
    );
}

#[test]
fn empty_strings_are_kept() {
    let doc = doc();
    let search = &doc["paths"]["/search"]["get"];
    assert_eq!(
        parameter(search, "q")["examples"]["empty_query"]["value"],
        ""
    );
    assert!(parameter(search, "limit").get("examples").is_none());
}

#[test]
fn every_example_value_matches_its_schema() {
    let doc = doc();
    let mut checked = 0;
    for path_item in doc["paths"].as_object().unwrap().values() {
        for operation in path_item.as_object().unwrap().values() {
            let mut targets: Vec<&Value> = Vec::new();
            for parameter in operation["parameters"].as_array().into_iter().flatten() {
                targets.push(parameter);
            }
            if operation["requestBody"].is_object() {
                targets.push(&operation["requestBody"]["content"]["application/json"]);
            }
            for response in operation["responses"].as_object().unwrap().values() {
                if response["content"]["application/json"].is_object() {
                    targets.push(&response["content"]["application/json"]);
                }
            }
            for target in targets {
                for (name, example) in target["examples"].as_object().into_iter().flatten() {
                    let problems = ras_spec_conformance::openapi_value_violations(
                        &doc,
                        &target["schema"],
                        &example["value"],
                    );
                    assert!(problems.is_empty(), "{name}: {problems:?}");
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 20);
}

#[tokio::test]
async fn the_server_answers_as_the_examples_say() {
    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(ItemServiceBuilder::new(Items).build())
        .unwrap();

    let ok = server
        .post("/api/items/42/rename")
        .add_query_param("notify", true)
        .json(&rename_body())
        .await;
    assert_eq!(ok.status_code().as_u16(), 200);
    assert_eq!(ok.json::<Item>(), spare_parts(42));

    let missing = server
        .post("/api/items/9999/rename")
        .json(&rename_body())
        .await;
    assert_eq!(missing.status_code().as_u16(), 404);
    assert_eq!(
        missing.json::<Value>(),
        json!({ "error": "Item not found" })
    );

    let created = server.post("/api/items").json(&rename_body()).await;
    assert_eq!(created.status_code().as_u16(), 201);

    let deleted = server.delete("/api/items/1").await;
    assert_eq!(deleted.status_code().as_u16(), 204);
    let gone = server.delete("/api/items/2").await;
    assert_eq!(gone.json::<Value>(), json!({ "error": "Already deleted" }));
}

// A request body the request type reads back differently.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Lossy {
    #[serde(default, rename(serialize = "out", deserialize = "in"))]
    value: String,
}

rest_service!({
    service_name: LossyRest,
    base_path: "/api",
    openapi: true,
    endpoints: [
        POST UNAUTHORIZED save (Lossy) -> () {
            examples: [ lossy { body: Lossy { value: "Ada".into() } } ],
        },
    ]
});

#[test]
fn a_body_that_reads_back_differently_fails_generation() {
    let error = try_generate_lossyrest_openapi().expect_err("the round trip catches it");
    assert_eq!(error.operation.as_deref(), Some("POST /save"));
    assert_eq!(error.example.as_deref(), Some("lossy"));
    assert_eq!(error.part.as_deref(), Some("body"));
    assert!(!error.to_string().contains("Ada"), "{error}");
}

// A failing document is served as a cached 500.
static FAILING_RUNS: AtomicUsize = AtomicUsize::new(0);

fn counted<T>(value: T) -> T {
    FAILING_RUNS.fetch_add(1, Ordering::SeqCst);
    value
}

rest_service!({
    service_name: FailingDocs,
    base_path: "/api",
    openapi: true,
    serve_docs: true,
    endpoints: [
        GET UNAUTHORIZED lookup () -> HashMap<String, u32> {
            examples: [
                tuple_keys { response: counted(HashMap::new()) },
            ],
        },
        GET UNAUTHORIZED broken () -> HashMap<(u32, u32), u32> {
            examples: [
                tuple_keys { response: HashMap::from([((1, 2), 3)]) },
            ],
        },
    ]
});

struct Failing;

#[async_trait::async_trait]
impl FailingDocsTrait for Failing {
    async fn get_lookup(&self) -> RestResult<HashMap<String, u32>> {
        Ok(RestResponse::ok(HashMap::new()))
    }
    async fn get_broken(&self) -> RestResult<HashMap<(u32, u32), u32>> {
        Ok(RestResponse::ok(HashMap::new()))
    }
}

#[tokio::test]
async fn a_failing_document_is_a_cached_500() {
    let error = try_generate_failingdocs_openapi().expect_err("tuple keys fail");
    assert_eq!(error.part.as_deref(), Some("response"));

    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(FailingDocsBuilder::new(Failing).build())
        .unwrap();
    let before = FAILING_RUNS.load(Ordering::SeqCst);
    for _ in 0..3 {
        let response = server.get("/api/docs/openapi.json").await;
        assert_eq!(response.status_code().as_u16(), 500);
        assert_eq!(response.text(), "OpenAPI document unavailable");
    }
    assert_eq!(FAILING_RUNS.load(Ordering::SeqCst), before + 1);
}

// Guarded docs: authorization runs before the cache, cold or warm.
static GUARDED_RUNS: AtomicUsize = AtomicUsize::new(0);

fn guarded_marker() -> u32 {
    GUARDED_RUNS.fetch_add(1, Ordering::SeqCst);
    1
}

rest_service!({
    service_name: GuardedRestDocs,
    base_path: "/api",
    openapi: true,
    serve_docs: true,
    docs_require_auth: true,
    endpoints: [
        GET UNAUTHORIZED count () -> u32 {
            examples: [ one { response: guarded_marker() } ],
        },
    ]
});

struct Guarded;

#[async_trait::async_trait]
impl GuardedRestDocsTrait for Guarded {
    async fn get_count(&self) -> RestResult<u32> {
        Ok(RestResponse::ok(1))
    }
}

struct TokenAuth;

impl AuthProvider for TokenAuth {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            if token == "good" {
                Ok(AuthenticatedUser {
                    user_id: "u".into(),
                    permissions: HashSet::new(),
                    metadata: None,
                })
            } else {
                Err(AuthError::InvalidToken)
            }
        })
    }
}

#[tokio::test]
async fn an_unauthorized_request_never_reaches_the_cache() {
    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(
            GuardedRestDocsBuilder::new(Guarded)
                .auth_provider(TokenAuth)
                .build(),
        )
        .unwrap();
    let path = "/api/docs/openapi.json";

    assert_eq!(server.get(path).await.status_code().as_u16(), 401);
    assert_eq!(GUARDED_RUNS.load(Ordering::SeqCst), 0);

    let authorized = server.get(path).authorization_bearer("good").await;
    assert_eq!(authorized.status_code().as_u16(), 200);
    assert_eq!(GUARDED_RUNS.load(Ordering::SeqCst), 1);

    assert_eq!(server.get(path).await.status_code().as_u16(), 401);
    let again = server.get(path).authorization_bearer("good").await;
    assert_eq!(
        again.json::<Value>()["info"]["title"],
        "GuardedRestDocs REST API"
    );
    assert_eq!(GUARDED_RUNS.load(Ordering::SeqCst), 1);
}

// Concurrent first requests build once; services keep their own documents.
static CONCURRENT_RUNS: AtomicUsize = AtomicUsize::new(0);

fn concurrent_marker() -> u32 {
    CONCURRENT_RUNS.fetch_add(1, Ordering::SeqCst);
    1
}

rest_service!({
    service_name: ConcurrentRestDocs,
    base_path: "/api",
    openapi: true,
    serve_docs: true,
    endpoints: [
        GET UNAUTHORIZED count () -> u32 {
            examples: [ one { response: concurrent_marker() } ],
        },
    ]
});

struct Concurrent;

#[async_trait::async_trait]
impl ConcurrentRestDocsTrait for Concurrent {
    async fn get_count(&self) -> RestResult<u32> {
        Ok(RestResponse::ok(1))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_requests_build_once_and_services_stay_apart() {
    let server = std::sync::Arc::new(
        axum_test::TestServer::builder()
            .mock_transport()
            .build(ConcurrentRestDocsBuilder::new(Concurrent).build())
            .unwrap(),
    );
    let requests = (0..8).map(|_| {
        let server = server.clone();
        tokio::spawn(async move { server.get("/api/docs/openapi.json").await.json::<Value>() })
    });
    for doc in futures::future::join_all(requests).await {
        assert_eq!(doc.unwrap()["info"]["title"], "ConcurrentRestDocs REST API");
    }
    assert_eq!(CONCURRENT_RUNS.load(Ordering::SeqCst), 1);

    let other = axum_test::TestServer::builder()
        .mock_transport()
        .build(ItemServiceBuilder::new(Items).build())
        .unwrap();
    let other_doc: Value = other.get("/api/docs/openapi.json").await.json();
    assert_eq!(other_doc["info"]["title"], "ItemService REST API");
}

// With `openapi` off, examples generate nothing: these values do not even have
// the declared types, and the service still compiles.
rest_service!({
    service_name: NoRestDocs,
    base_path: "/api",
    endpoints: [
        GET UNAUTHORIZED count () -> u32 {
            examples: [ wrong_type { response: "not a u32" } ],
        },
    ]
});

#[test]
fn examples_without_openapi_generate_nothing() {
    fn _service_exists<T: NoRestDocsTrait>() {}
}
