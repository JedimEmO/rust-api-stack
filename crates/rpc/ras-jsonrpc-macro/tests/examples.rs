//! Authored examples appear in the OpenRPC document as example pairings and
//! `x-ras-error-examples`, match their schemas, and fail generation loudly
//! when they cannot be serialized faithfully.

use ras_jsonrpc_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateUser {
    name: String,
    email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct User {
    id: u32,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmailTaken {
    field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Rename {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenameV1 {
    title: String,
}

pub struct RenameCompat;

impl ras_jsonrpc_core::VersionMigration<RenameV1, Rename> for RenameCompat {
    type Error = std::convert::Infallible;
    fn migrate(value: RenameV1) -> Result<Rename, Self::Error> {
        Ok(Rename { name: value.title })
    }
}

impl ras_jsonrpc_core::VersionMigration<User, RenameV1> for RenameCompat {
    type Error = std::convert::Infallible;
    fn migrate(value: User) -> Result<RenameV1, Self::Error> {
        Ok(RenameV1 { title: value.name })
    }
}

fn ada() -> User {
    User {
        id: 1,
        name: "Ada".into(),
    }
}

jsonrpc_service!({
    service_name: ExampleService,
    openrpc: true,
    explorer: true,
    methods: [
        /// Create a user account.
        UNAUTHORIZED create_user(CreateUser) -> User {
            examples: [
                /// A user with only the required fields.
                ///
                /// The nickname is left out.
                basic {
                    params: CreateUser { name: "Ada".into(), email: "ada@example.com".into(), nickname: None },
                    result: ada(),
                },
                /// The e-mail address is already registered.
                duplicate_email {
                    params: CreateUser { name: "Ada".into(), email: "taken@example.com".into(), nickname: None },
                    error: { code: 1001, message: "Email already registered", data: EmailTaken { field: "email".into() } },
                },
                another_duplicate {
                    params: CreateUser { name: "Bob".into(), email: "taken@example.com".into(), nickname: None },
                    error: { code: 1001, message: "Second message for the same code" },
                },
                invalid {
                    params: CreateUser { name: "".into(), email: "x".into(), nickname: None },
                    error: { code: -32602, message: "Invalid params" },
                },
            ],
        },
        WITH_PERMISSIONS(["admin"]) bulk(Vec<u32>) -> u32 {
            examples: [
                three { params: vec![1, 2, 3], result: 3 },
            ],
        },
        UNAUTHORIZED ping(()) -> String {
            examples: [
                pong { result: "pong".to_string() },
                closed { error: { code: 503, message: "Maintenance" } },
            ],
        },
        UNAUTHORIZED rename(Rename) -> User {
            version: v2,
            wire: "rename.v2",
            versions: [
                v1 {
                    wire: "rename.v1",
                    request: RenameV1,
                    response: RenameV1,
                    migration: RenameCompat,
                },
            ],
            examples: [
                rename_ada { params: Rename { name: "Ada".into() }, result: ada() },
            ],
        },
    ]
});

struct Handlers;

impl ExampleServiceTrait for Handlers {
    async fn create_user(
        &self,
        request: CreateUser,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        Ok(User {
            id: 1,
            name: request.name,
        })
    }
    async fn bulk(
        &self,
        _user: &AuthenticatedUser,
        request: Vec<u32>,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        Ok(request.len() as u32)
    }
    async fn ping(&self, _request: ()) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("pong".into())
    }
    async fn rename(
        &self,
        request: Rename,
    ) -> Result<User, Box<dyn std::error::Error + Send + Sync>> {
        Ok(User {
            id: 1,
            name: request.name,
        })
    }
}

fn doc() -> Value {
    try_generate_exampleservice_openrpc().expect("document builds")
}

fn method<'a>(doc: &'a Value, name: &str) -> &'a Value {
    doc["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == name)
        .unwrap_or_else(|| panic!("method {name} missing"))
}

/// Rebuilds the whole request from a pairing's `params`, the way a by-name or
/// raw call would send it.
fn request_value(method: &Value, params: &Value) -> Value {
    if method["x-ras-params-raw"] == true {
        params[0]["value"].clone()
    } else {
        let object: serde_json::Map<String, Value> = params
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["name"].as_str().unwrap().to_string(), p["value"].clone()))
            .collect();
        Value::Object(object)
    }
}

#[test]
fn document_with_examples_conforms() {
    ras_spec_conformance::assert_openrpc_conforms(&doc());
}

#[test]
fn success_examples_become_pairings_with_their_docs() {
    let doc = doc();
    let create = method(&doc, "create_user");
    let examples = create["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 1, "only success examples are pairings");
    assert_eq!(
        examples[0],
        json!({
            "name": "basic",
            "description": "A user with only the required fields.\n\nThe nickname is left out.",
            "params": [
                { "name": "email", "value": "ada@example.com" },
                { "name": "name", "value": "Ada" }
            ],
            "result": { "name": "result", "value": { "id": 1, "name": "Ada" } }
        })
    );
}

#[test]
fn error_examples_go_to_the_extension_and_the_errors_list() {
    let doc = doc();
    let create = method(&doc, "create_user");
    let error_examples = create["x-ras-error-examples"].as_array().unwrap();
    assert_eq!(error_examples.len(), 3);
    assert_eq!(error_examples[0]["name"], "duplicate_email");
    assert_eq!(
        error_examples[0]["description"],
        "The e-mail address is already registered."
    );
    assert_eq!(
        error_examples[0]["error"],
        json!({ "code": 1001, "message": "Email already registered", "data": { "field": "email" } })
    );
    assert!(error_examples[1]["error"].get("data").is_none());

    // Built-ins first, then each application code once, with the first message.
    assert_eq!(
        create["errors"],
        json!([
            { "$ref": "#/components/errors/InvalidParams" },
            { "$ref": "#/components/errors/InternalError" },
            { "code": 1001, "message": "Email already registered" }
        ])
    );
}

#[test]
fn raw_and_unit_methods_get_matching_pairings() {
    let doc = doc();
    let bulk = method(&doc, "bulk");
    assert_eq!(bulk["x-ras-params-raw"], true);
    assert_eq!(
        bulk["examples"][0]["params"],
        json!([{ "name": "params", "value": [1, 2, 3] }])
    );
    assert_eq!(
        bulk["errors"],
        json!([
            { "$ref": "#/components/errors/InvalidParams" },
            { "$ref": "#/components/errors/AuthenticationRequired" },
            { "$ref": "#/components/errors/InsufficientPermissions" },
            { "$ref": "#/components/errors/TokenExpired" },
            { "$ref": "#/components/errors/InternalError" }
        ])
    );

    let ping = method(&doc, "ping");
    assert_eq!(ping["examples"][0]["params"], json!([]));
    assert_eq!(ping["x-ras-error-examples"][0]["params"], json!([]));
    assert_eq!(
        ping["errors"],
        json!([
            { "$ref": "#/components/errors/InternalError" },
            { "code": 503, "message": "Maintenance" }
        ])
    );
}

#[test]
fn only_the_current_version_has_examples() {
    let doc = doc();
    assert_eq!(
        method(&doc, "rename.v2")["examples"][0]["name"],
        "rename_ada"
    );
    let legacy = method(&doc, "rename.v1");
    assert!(legacy.get("examples").is_none());
    assert!(legacy.get("x-ras-error-examples").is_none());
}

#[test]
fn every_example_value_matches_its_schema() {
    let doc = doc();
    let mut checked = 0;
    for method in doc["methods"].as_array().unwrap() {
        let request_schema = method.get("x-ras-params-schema");
        let pairings = method["examples"].as_array().cloned().unwrap_or_default();
        let errors = method["x-ras-error-examples"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for example in pairings.iter().chain(errors.iter()) {
            let label = format!("{} {}", method["name"], example["name"]);
            if let Some(schema) = request_schema {
                let request = request_value(method, &example["params"]);
                let problems =
                    ras_spec_conformance::openrpc_value_violations(&doc, schema, &request);
                assert!(problems.is_empty(), "{label} params: {problems:?}");
                checked += 1;
            }
            if let Some(result) = example.get("result") {
                let problems = ras_spec_conformance::openrpc_value_violations(
                    &doc,
                    &method["result"]["schema"],
                    &result["value"],
                );
                assert!(problems.is_empty(), "{label} result: {problems:?}");
                checked += 1;
            }
        }
    }
    // create_user: 4 requests and 1 result; bulk: 1 and 1; ping: 1 result;
    // rename.v2: 1 and 1.
    assert_eq!(checked, 10);
}

#[tokio::test]
async fn examples_describe_calls_the_server_answers_as_documented() {
    let router = ExampleServiceBuilder::new(Handlers)
        .base_url("/rpc")
        .auth_provider(TokenAuth)
        .build()
        .expect("router builds");
    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();
    let doc = doc();
    for name in ["create_user", "rename.v2"] {
        let method = method(&doc, name);
        let example = &method["examples"][0];
        let params = request_value(method, &example["params"]);
        let response: Value = server
            .post("/rpc")
            .json(&json!({ "jsonrpc": "2.0", "method": name, "params": params, "id": 1 }))
            .await
            .json();
        assert_eq!(
            response["result"], example["result"]["value"],
            "{name}: {response}"
        );
    }
}

// A request example the request type reads back differently.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Lossy {
    #[serde(default, rename(serialize = "out", deserialize = "in"))]
    value: String,
}

jsonrpc_service!({
    service_name: LossyService,
    openrpc: true,
    methods: [
        UNAUTHORIZED save(Lossy) -> () {
            examples: [
                lossy { params: Lossy { value: "Ada".into() }, result: () },
            ],
        },
    ]
});

#[test]
fn a_request_example_that_reads_back_differently_fails_generation() {
    let error = try_generate_lossyservice_openrpc().expect_err("the round trip catches it");
    assert_eq!(error.operation.as_deref(), Some("save"));
    assert_eq!(error.example.as_deref(), Some("lossy"));
    assert_eq!(error.part.as_deref(), Some("params"));
    assert!(error.cause.contains("reads back differently"), "{error}");
    assert!(
        !error.to_string().contains("Ada"),
        "the value never appears: {error}"
    );
}

#[test]
#[should_panic(expected = "example `lossy`")]
fn the_infallible_generator_panics_with_the_location() {
    let _ = generate_lossyservice_openrpc();
}

// An example whose data cannot become JSON at all.
jsonrpc_service!({
    service_name: UnserializableService,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED lookup(()) -> u32 {
            examples: [
                tuple_keys {
                    error: { code: 1002, message: "Not found", data: counted(HashMap::from([((1u32, 2u32), 3u32)])) },
                },
            ],
        },
    ]
});

static UNSERIALIZABLE_RUNS: AtomicUsize = AtomicUsize::new(0);

fn counted<T>(value: T) -> T {
    UNSERIALIZABLE_RUNS.fetch_add(1, Ordering::SeqCst);
    value
}

struct Lookup;

impl UnserializableServiceTrait for Lookup {
    async fn lookup(&self, _request: ()) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        Ok(0)
    }
}

#[tokio::test]
async fn a_failing_document_is_a_cached_500() {
    let error = try_generate_unserializableservice_openrpc().expect_err("tuple keys fail");
    assert_eq!(error.part.as_deref(), Some("error data"));
    assert!(error.cause.contains("does not serialize"), "{error}");

    let router = UnserializableServiceBuilder::new(Lookup)
        .base_url("/rpc")
        .build()
        .expect("router builds");
    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();
    let before = UNSERIALIZABLE_RUNS.load(Ordering::SeqCst);
    for _ in 0..3 {
        let response = server.get("/rpc/explorer/openrpc.json").await;
        assert_eq!(response.status_code().as_u16(), 500);
        assert_eq!(response.text(), "OpenRPC document unavailable");
    }
    assert_eq!(UNSERIALIZABLE_RUNS.load(Ordering::SeqCst), before + 1);
}

// A service whose document is behind authentication.
static GUARDED_RUNS: AtomicUsize = AtomicUsize::new(0);

fn guarded_marker() -> String {
    GUARDED_RUNS.fetch_add(1, Ordering::SeqCst);
    "pong".into()
}

jsonrpc_service!({
    service_name: GuardedDocs,
    openrpc: true,
    explorer: true,
    docs_require_auth: true,
    methods: [
        UNAUTHORIZED ping(()) -> String {
            examples: [ pong { result: guarded_marker() } ],
        },
    ]
});

struct Guarded;

impl GuardedDocsTrait for Guarded {
    async fn ping(&self, _request: ()) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("pong".into())
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
    let router = GuardedDocsBuilder::new(Guarded)
        .base_url("/rpc")
        .auth_provider(TokenAuth)
        .build()
        .expect("router builds");
    let server = axum_test::TestServer::builder()
        .mock_transport()
        .build(router)
        .unwrap();
    let path = "/rpc/explorer/openrpc.json";

    let cold = server.get(path).await;
    assert_eq!(cold.status_code().as_u16(), 401);
    assert_eq!(
        GUARDED_RUNS.load(Ordering::SeqCst),
        0,
        "a cold unauthorized request builds nothing"
    );

    let authorized = server.get(path).authorization_bearer("good").await;
    assert_eq!(authorized.status_code().as_u16(), 200);
    assert_eq!(GUARDED_RUNS.load(Ordering::SeqCst), 1);

    let warm = server.get(path).await;
    assert_eq!(
        warm.status_code().as_u16(),
        401,
        "a warm cache is still guarded"
    );
    let again = server.get(path).authorization_bearer("good").await;
    assert_eq!(
        again.json::<Value>()["info"]["title"],
        "GuardedDocs JSON-RPC API"
    );
    assert_eq!(GUARDED_RUNS.load(Ordering::SeqCst), 1);
}

// Concurrent first requests build the document once.
static CONCURRENT_RUNS: AtomicUsize = AtomicUsize::new(0);

fn concurrent_marker() -> String {
    CONCURRENT_RUNS.fetch_add(1, Ordering::SeqCst);
    "pong".into()
}

jsonrpc_service!({
    service_name: ConcurrentDocs,
    openrpc: true,
    explorer: true,
    methods: [
        UNAUTHORIZED ping(()) -> String {
            examples: [ pong { result: concurrent_marker() } ],
        },
    ]
});

struct Concurrent;

impl ConcurrentDocsTrait for Concurrent {
    async fn ping(&self, _request: ()) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("pong".into())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_requests_build_once_and_services_stay_apart() {
    let concurrent = std::sync::Arc::new(
        axum_test::TestServer::builder()
            .mock_transport()
            .build(
                ConcurrentDocsBuilder::new(Concurrent)
                    .base_url("/rpc")
                    .build()
                    .unwrap(),
            )
            .unwrap(),
    );
    let requests = (0..8).map(|_| {
        let server = concurrent.clone();
        tokio::spawn(async move {
            server
                .get("/rpc/explorer/openrpc.json")
                .await
                .json::<Value>()
        })
    });
    for doc in futures::future::join_all(requests).await {
        assert_eq!(doc.unwrap()["info"]["title"], "ConcurrentDocs JSON-RPC API");
    }
    assert_eq!(CONCURRENT_RUNS.load(Ordering::SeqCst), 1);

    let other = axum_test::TestServer::builder()
        .mock_transport()
        .build(
            ExampleServiceBuilder::new(Handlers)
                .base_url("/rpc")
                .auth_provider(TokenAuth)
                .build()
                .unwrap(),
        )
        .unwrap();
    let other_doc: Value = other.get("/rpc/explorer/openrpc.json").await.json();
    assert_eq!(other_doc["info"]["title"], "ExampleService JSON-RPC API");
}

// With `openrpc` off, examples generate nothing: these values are not even of
// the request and response types, and the service still compiles.
jsonrpc_service!({
    service_name: NoDocs,
    methods: [
        UNAUTHORIZED double(u32) -> u32 {
            examples: [ wrong_types { params: "not a u32", result: "nor this" } ],
        },
    ]
});

#[test]
fn examples_without_openrpc_generate_nothing() {
    fn _service_exists<T: NoDocsTrait>() {}
}

// Helpers whose names match locals the generated code declares around examples.
mod clashing {
    pub fn context() -> String {
        "from context()".into()
    }
    pub fn generators() -> u32 {
        7
    }
    pub fn value() -> String {
        "from value()".into()
    }
}
use clashing::{context, generators, value};

jsonrpc_service!({
    service_name: HygieneService,
    openrpc: true,
    methods: [
        UNAUTHORIZED echo(String) -> String {
            examples: [
                uses_helpers { params: context(), result: value() },
                more_helpers { params: value(), error: { code: 1001, message: "x", data: generators() } },
            ],
        },
    ]
});

#[test]
fn examples_can_call_helpers_named_like_generated_locals() {
    let doc = try_generate_hygieneservice_openrpc().expect("document builds");
    let echo = &doc["methods"][0];
    assert_eq!(echo["examples"][0]["params"][0]["value"], "from context()");
    assert_eq!(echo["examples"][0]["result"]["value"], "from value()");
    assert_eq!(echo["x-ras-error-examples"][0]["error"]["data"], 7);
}
