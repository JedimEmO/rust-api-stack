//! The generated OpenRPC document is valid OpenRPC 1.3, and its references
//! point at the schemas they should.

use ras_jsonrpc_macro::jsonrpc_service;
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
#[serde(rename_all = "camelCase")]
pub struct Lookup {
    user_id: u32,
    include_inactive: Option<bool>,
}

jsonrpc_service!({
    service_name: ShapesService,
    openrpc: true,
    methods: [
        /// Store a customer.
        UNAUTHORIZED create_customer(Customer) -> Customer,
        UNAUTHORIZED grow_tree(TreeNode) -> TreeNode,
        UNAUTHORIZED page(Envelope<Customer>) -> Envelope<Vec<Customer>>,
        UNAUTHORIZED link_billing(billing::Account) -> (),
        UNAUTHORIZED link_crm(crm::Account) -> (),
        UNAUTHORIZED echo_renamed(Renamed) -> Renamed,
        UNAUTHORIZED draw(Shape) -> f64,
        UNAUTHORIZED lookup(Lookup) -> Vec<Customer>,
        UNAUTHORIZED ping(()) -> (),
        UNAUTHORIZED count(u32) -> HashMap<String, u32>,
    ]
});

fn doc() -> Value {
    generate_shapesservice_openrpc()
}

fn method<'a>(doc: &'a Value, name: &str) -> &'a Value {
    doc["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == name)
        .unwrap_or_else(|| panic!("method {name} missing"))
}

/// The schema describing a method's whole request.
fn request_schema<'a>(doc: &'a Value, name: &str) -> &'a Value {
    let method = method(doc, name);
    method
        .get("x-ras-params-schema")
        .unwrap_or_else(|| &method["params"][0]["schema"])
}

fn result_schema<'a>(doc: &'a Value, name: &str) -> &'a Value {
    &method(doc, name)["result"]["schema"]
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
fn document_conforms_to_openrpc() {
    ras_spec_conformance::assert_openrpc_conforms(&doc());
}

#[test]
fn nested_struct_reference_reaches_the_struct() {
    let doc = doc();
    let customer = request_schema(&doc, "create_customer");
    let delivery = property(&doc, customer, "delivery");
    assert!(has_property(&doc, delivery, "street"));
    assert_eq!(delivery["description"], "Where orders are delivered.");
    let billing = property(&doc, customer, "billing");
    assert!(
        has_property(&doc, billing, "street")
            || deref(&doc, billing)["anyOf"]
                .as_array()
                .is_some_and(|v| v.iter().any(|s| has_property(&doc, s, "street")))
    );
}

#[test]
fn recursive_reference_points_back_at_the_type() {
    let doc = doc();
    let tree = request_schema(&doc, "grow_tree");
    let children = property(&doc, tree, "children");
    let item = &deref(&doc, children)["items"];
    assert!(has_property(&doc, item, "children"));
    assert!(has_property(&doc, item, "label"));
}

#[test]
fn generic_types_resolve_to_their_instantiation() {
    let doc = doc();
    let request = request_schema(&doc, "page");
    assert!(has_property(
        &doc,
        property(&doc, request, "data"),
        "delivery"
    ));
    let response = result_schema(&doc, "page");
    let data = property(&doc, response, "data");
    assert_eq!(deref(&doc, data)["type"], "array");
}

#[test]
fn same_named_types_stay_distinct() {
    let doc = doc();
    assert!(has_property(
        &doc,
        request_schema(&doc, "link_billing"),
        "iban"
    ));
    assert!(has_property(
        &doc,
        request_schema(&doc, "link_crm"),
        "owner"
    ));
}

#[test]
fn requests_and_responses_use_their_own_contract() {
    let doc = doc();
    assert!(has_property(
        &doc,
        request_schema(&doc, "echo_renamed"),
        "in"
    ));
    assert!(has_property(
        &doc,
        result_schema(&doc, "echo_renamed"),
        "out"
    ));
}

#[test]
fn renamed_fields_use_wire_names() {
    let doc = doc();
    assert!(has_property(&doc, request_schema(&doc, "lookup"), "userId"));
}
