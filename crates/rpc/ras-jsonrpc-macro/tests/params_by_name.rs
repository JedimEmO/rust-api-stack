//! OpenRPC parameter descriptors describe calls the server accepts.
//!
//! For each request shape, a call is built from the generated descriptors alone
//! and sent through the real dispatch.

use ras_jsonrpc_macro::jsonrpc_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CamelCase {
    user_id: u32,
    display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Strict {
    id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Sparse {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Inner {
    note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Flattened {
    id: u32,
    #[serde(flatten)]
    inner: Inner,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlattenedMap {
    id: u32,
    #[serde(flatten)]
    rest: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub enum Kind {
    Small { size: u8 },
    Large { size: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlattenedEnum {
    id: u32,
    #[serde(flatten)]
    kind: Kind,
}

/// `a` may only be sent together with `b`, which named descriptors cannot say.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("dependencies" = { "a": ["b"] }))]
pub struct Dependent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    a: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    b: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Generic<T> {
    value: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Newtype(Strict);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Empty {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct AllOptional {
    a: Option<u32>,
    b: Option<String>,
}

pub struct AllOptionalCompat;

impl ras_jsonrpc_core::VersionMigration<Empty, AllOptional> for AllOptionalCompat {
    type Error = std::convert::Infallible;

    fn migrate(_value: Empty) -> Result<AllOptional, Self::Error> {
        Ok(AllOptional::default())
    }
}

impl ras_jsonrpc_core::VersionMigration<AllOptional, Empty> for AllOptionalCompat {
    type Error = std::convert::Infallible;

    fn migrate(_value: AllOptional) -> Result<Empty, Self::Error> {
        Ok(Empty {})
    }
}

jsonrpc_service!({
    service_name: ParamsService,
    openrpc: true,
    methods: [
        UNAUTHORIZED camel(CamelCase) -> CamelCase,
        UNAUTHORIZED strict(Strict) -> Strict,
        UNAUTHORIZED sparse(Sparse) -> Sparse,
        UNAUTHORIZED flattened(Flattened) -> Flattened,
        UNAUTHORIZED flattened_map(FlattenedMap) -> FlattenedMap,
        UNAUTHORIZED flattened_enum(FlattenedEnum) -> FlattenedEnum,
        UNAUTHORIZED dependent(Dependent) -> Dependent,
        UNAUTHORIZED generic(Generic<u32>) -> Generic<u32>,
        UNAUTHORIZED newtype(Newtype) -> Newtype,
        UNAUTHORIZED list(Vec<u32>) -> Vec<u32>,
        UNAUTHORIZED unit(()) -> (),
        UNAUTHORIZED empty(Empty) -> Empty,
        UNAUTHORIZED optional(AllOptional) -> AllOptional {
            version: v2,
            wire: "optional.v2",
            versions: [
                v1 {
                    wire: "optional.v1",
                    request: Empty,
                    response: Empty,
                    migration: AllOptionalCompat,
                },
            ],
        },
    ]
});

type HandlerResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Echo;

impl ParamsServiceTrait for Echo {
    async fn camel(&self, request: CamelCase) -> HandlerResult<CamelCase> {
        Ok(request)
    }
    async fn strict(&self, request: Strict) -> HandlerResult<Strict> {
        Ok(request)
    }
    async fn sparse(&self, request: Sparse) -> HandlerResult<Sparse> {
        Ok(request)
    }
    async fn flattened(&self, request: Flattened) -> HandlerResult<Flattened> {
        Ok(request)
    }
    async fn flattened_map(&self, request: FlattenedMap) -> HandlerResult<FlattenedMap> {
        Ok(request)
    }
    async fn flattened_enum(&self, request: FlattenedEnum) -> HandlerResult<FlattenedEnum> {
        Ok(request)
    }
    async fn dependent(&self, request: Dependent) -> HandlerResult<Dependent> {
        Ok(request)
    }
    async fn generic(&self, request: Generic<u32>) -> HandlerResult<Generic<u32>> {
        Ok(request)
    }
    async fn newtype(&self, request: Newtype) -> HandlerResult<Newtype> {
        Ok(request)
    }
    async fn list(&self, request: Vec<u32>) -> HandlerResult<Vec<u32>> {
        Ok(request)
    }
    async fn unit(&self, request: ()) -> HandlerResult<()> {
        Ok(request)
    }
    async fn empty(&self, request: Empty) -> HandlerResult<Empty> {
        Ok(request)
    }
    async fn optional(&self, request: AllOptional) -> HandlerResult<AllOptional> {
        Ok(request)
    }
}

fn server() -> axum_test::TestServer {
    let router = ParamsServiceBuilder::new(Echo)
        .base_url("/rpc")
        .build()
        .expect("router builds");
    axum_test::TestServer::builder()
        .mock_transport()
        .build(router)
        .expect("test server")
}

/// Sends a request; `params: None` leaves the field out.
async fn call(server: &axum_test::TestServer, method: &str, params: Option<Value>) -> Value {
    let mut body = json!({ "jsonrpc": "2.0", "method": method, "id": 1 });
    if let Some(params) = params {
        body["params"] = params;
    }
    server.post("/rpc").json(&body).await.json()
}

fn method(name: &str) -> Value {
    generate_paramsservice_openrpc()["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == name)
        .unwrap_or_else(|| panic!("method {name} missing"))
        .clone()
}

/// Builds a by-name call for `value` from `name`'s descriptors, checking that
/// the descriptors cover exactly what the value needs.
fn by_name_params(name: &str, value: &Value) -> Value {
    let method = method(name);
    assert_eq!(method["paramStructure"], "by-name", "{name}: {method}");
    assert!(method.get("x-ras-params-raw").is_none(), "{name}");
    assert!(method["x-ras-params-schema"].is_object(), "{name}");

    let descriptors = method["params"].as_array().unwrap();
    let names: BTreeSet<&str> = descriptors
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    let required: BTreeSet<&str> = descriptors
        .iter()
        .filter(|d| d["required"] == true)
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    let keys: BTreeSet<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        keys.is_subset(&names),
        "{name}: {keys:?} not all described by {names:?}"
    );
    assert!(
        required.is_subset(&keys),
        "{name}: required {required:?} missing from {keys:?}"
    );

    let mut params = serde_json::Map::new();
    for descriptor in descriptors {
        let field = descriptor["name"].as_str().unwrap();
        if let Some(field_value) = value.get(field) {
            params.insert(field.to_string(), field_value.clone());
        }
    }
    Value::Object(params)
}

fn assert_raw(name: &str) {
    let method = method(name);
    assert_eq!(method["x-ras-params-raw"], true, "{name}: {method}");
    assert!(method.get("paramStructure").is_none(), "{name}");
    let descriptors = method["params"].as_array().unwrap();
    assert_eq!(descriptors.len(), 1, "{name}");
    assert_eq!(descriptors[0]["name"], "params", "{name}");
    assert_eq!(
        descriptors[0]["schema"], method["x-ras-params-schema"],
        "{name}"
    );
}

#[tokio::test]
async fn named_requests_are_called_from_their_descriptors() {
    let server = server();
    let cases = [
        ("camel", json!({ "userId": 7, "displayName": "Ada" })),
        ("strict", json!({ "id": 3 })),
        ("sparse", json!({ "name": "Ada" })),
        ("sparse", json!({ "name": "Ada", "nickname": "A" })),
        ("flattened", json!({ "id": 1, "note": "n" })),
        ("generic", json!({ "value": 9 })),
        ("newtype", json!({ "id": 4 })),
        ("empty", json!({})),
    ];
    for (name, value) in cases {
        let params = by_name_params(name, &value);
        let response = call(&server, name, Some(params)).await;
        assert_eq!(response["result"], value, "{name}: {response}");
    }
}

#[tokio::test]
async fn shapes_descriptors_cannot_carry_are_raw_and_called_with_the_value() {
    let server = server();
    let cases = [
        ("flattened_map", json!({ "id": 1, "anything": true })),
        (
            "flattened_enum",
            json!({ "id": 1, "kind": "Small", "size": 2 }),
        ),
        ("dependent", json!({ "a": "x", "b": "y" })),
        ("list", json!([1, 2])),
    ];
    for (name, value) in cases {
        assert_raw(name);
        let response = call(&server, name, Some(value.clone())).await;
        assert_eq!(response["result"], value, "{name}: {response}");
    }
}

#[tokio::test]
async fn descriptors_carry_field_docs_and_requiredness() {
    let sparse = method("sparse");
    let params = sparse["params"].as_array().unwrap();
    let nickname = params.iter().find(|p| p["name"] == "nickname").unwrap();
    assert_eq!(nickname["required"], false);
    let name = params.iter().find(|p| p["name"] == "name").unwrap();
    assert_eq!(name["required"], true);
}

#[tokio::test]
async fn unit_requests_take_no_params() {
    let unit = method("unit");
    assert_eq!(unit["params"], json!([]));
    assert!(unit.get("x-ras-params-schema").is_none());
    let server = server();
    for params in [None, Some(Value::Null)] {
        let response = call(&server, "unit", params.clone()).await;
        assert_eq!(response["result"], Value::Null, "{params:?}: {response}");
        assert!(response.get("error").is_none(), "{params:?}: {response}");
    }
}

#[tokio::test]
async fn document_with_by_name_params_still_conforms() {
    ras_spec_conformance::assert_openrpc_conforms(&generate_paramsservice_openrpc());
}

/// What each way of leaving out or emptying `params` does, for a struct
/// without fields and one whose fields are all optional, on both the canonical
/// and the legacy dispatch path.
#[tokio::test]
async fn absent_null_empty_object_and_empty_array_params() {
    let server = server();
    let ok = |response: &Value| response.get("error").is_none();
    let invalid = |response: &Value| response["error"]["code"] == -32602;

    for method in ["empty", "optional.v2", "optional.v1"] {
        let absent = call(&server, method, None).await;
        assert!(ok(&absent), "{method} absent: {absent}");

        let null = call(&server, method, Some(Value::Null)).await;
        assert!(invalid(&null), "{method} null: {null}");

        let object = call(&server, method, Some(json!({}))).await;
        assert!(ok(&object), "{method} {{}}: {object}");
    }

    // serde decodes a struct from a sequence by position: an empty one fills a
    // struct without fields, but not one with fields, even optional ones.
    let empty_array = call(&server, "empty", Some(json!([]))).await;
    assert!(ok(&empty_array), "empty []: {empty_array}");
    let optional_array = call(&server, "optional.v2", Some(json!([]))).await;
    assert!(invalid(&optional_array), "optional.v2 []: {optional_array}");
}
