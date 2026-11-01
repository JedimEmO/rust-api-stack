//! The generated OpenAPI document is valid OpenAPI 3.0, and its references
//! point at the schemas they should.

use ras_rest_macro::rest_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A postal address.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Address {
    street: String,
    city: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Customer {
    name: String,
    /// Where orders are delivered.
    delivery: Address,
    billing: Option<Address>,
    nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreeNode {
    label: String,
    children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Envelope<T> {
    data: T,
    total: u32,
}

pub mod billing {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct Account {
        pub iban: String,
    }
}

pub mod crm {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct Account {
        pub owner: String,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Renamed {
    #[serde(rename(serialize = "out", deserialize = "in"))]
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub enum Shape {
    Circle { radius: f64 },
    Square { side: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum Status {
    Active,
    Archived,
}

rest_service!({
    service_name: ShapesRestService,
    base_path: "/api",
    openapi: true,
    endpoints: [
        /// Store a customer.
        POST UNAUTHORIZED customers (Customer) -> Customer,
        GET UNAUTHORIZED customers/{id: u32} ? verbose: Option<bool> () -> Customer,
        POST UNAUTHORIZED trees (TreeNode) -> TreeNode,
        POST UNAUTHORIZED pages (Envelope<Customer>) -> Envelope<Vec<Customer>>,
        POST UNAUTHORIZED billing (billing::Account) -> (),
        POST UNAUTHORIZED crm (crm::Account) -> (),
        POST UNAUTHORIZED renamed (Renamed) -> Renamed,
        POST UNAUTHORIZED shapes (Shape) -> f64,
        GET UNAUTHORIZED statuses ? status: Status () -> HashMap<String, u32>,
    ]
});

fn doc() -> Value {
    generate_shapesrestservice_openapi()
}

fn operation<'a>(doc: &'a Value, method: &str, path: &str) -> &'a Value {
    let operation = &doc["paths"][path][method];
    assert!(operation.is_object(), "{method} {path} missing");
    operation
}

fn body_schema<'a>(doc: &'a Value, path: &str) -> &'a Value {
    &operation(doc, "post", path)["requestBody"]["content"]["application/json"]["schema"]
}

fn response_schema<'a>(doc: &'a Value, method: &str, path: &str) -> &'a Value {
    &operation(doc, method, path)["responses"]["200"]["content"]["application/json"]["schema"]
}

/// Follows `$ref`s and single-reference `allOf` wrappers to the schema they stand for.
fn deref<'a>(doc: &'a Value, mut schema: &'a Value) -> &'a Value {
    for _ in 0..16 {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            schema = ras_spec_conformance::resolve_ref(doc, reference)
                .unwrap_or_else(|| panic!("{reference} does not resolve"));
        } else if let Some([only]) = schema
            .get("allOf")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
        {
            schema = only;
        } else {
            return schema;
        }
    }
    panic!("reference chain too long")
}

fn property<'a>(doc: &'a Value, schema: &'a Value, name: &str) -> &'a Value {
    deref(doc, schema)["properties"]
        .get(name)
        .unwrap_or_else(|| panic!("property {name} missing in {schema}"))
}

fn has_property(doc: &Value, schema: &Value, name: &str) -> bool {
    deref(doc, schema)["properties"].get(name).is_some()
}

#[test]
fn document_conforms_to_openapi_3_0() {
    ras_spec_conformance::assert_openapi3_conforms(&doc());
}

#[test]
fn nested_struct_reference_reaches_the_struct() {
    let doc = doc();
    let customer = body_schema(&doc, "/customers");
    let delivery = property(&doc, customer, "delivery");
    assert!(has_property(&doc, delivery, "street"));
    assert_eq!(delivery["description"], "Where orders are delivered.");
    let nickname = deref(&doc, property(&doc, customer, "nickname"));
    assert_eq!(nickname["type"], "string");
    assert_eq!(nickname["nullable"], true);
}

#[test]
fn recursive_reference_points_back_at_the_type() {
    let doc = doc();
    let tree = body_schema(&doc, "/trees");
    let children = property(&doc, tree, "children");
    let item = &deref(&doc, children)["items"];
    assert!(has_property(&doc, item, "children"));
    assert!(has_property(&doc, item, "label"));
}

#[test]
fn generic_types_resolve_to_their_instantiation() {
    let doc = doc();
    let request = body_schema(&doc, "/pages");
    assert!(has_property(
        &doc,
        property(&doc, request, "data"),
        "delivery"
    ));
    let response = response_schema(&doc, "post", "/pages");
    let data = property(&doc, response, "data");
    assert_eq!(deref(&doc, data)["type"], "array");
}

#[test]
fn same_named_types_stay_distinct() {
    let doc = doc();
    assert!(has_property(&doc, body_schema(&doc, "/billing"), "iban"));
    assert!(has_property(&doc, body_schema(&doc, "/crm"), "owner"));
}

#[test]
fn requests_and_responses_use_their_own_contract() {
    let doc = doc();
    assert!(has_property(&doc, body_schema(&doc, "/renamed"), "in"));
    assert!(has_property(
        &doc,
        response_schema(&doc, "post", "/renamed"),
        "out"
    ));
}

#[test]
fn parameters_describe_their_types() {
    let doc = doc();
    let get = operation(&doc, "get", "/customers/{id}");
    let params = get["parameters"].as_array().unwrap();
    let id = params.iter().find(|p| p["name"] == "id").unwrap();
    assert_eq!(deref(&doc, &id["schema"])["type"], "integer");
    let status = &operation(&doc, "get", "/statuses")["parameters"][0];
    assert!(deref(&doc, &status["schema"])["enum"].is_array());
}
