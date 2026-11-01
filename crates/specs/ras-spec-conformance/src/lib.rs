//! Checks generated OpenRPC and OpenAPI documents against the official schemas.
//!
//! ```ignore
//! let doc = generate_userservice_openrpc();
//! ras_spec_conformance::assert_openrpc_conforms(&doc);
//! ```
//!
//! Each `*_violations` function returns one readable line per problem, so a
//! failing test shows every problem at once instead of the first.

use jsonschema::{Draft, Retrieve, Uri, Validator};
use serde_json::Value;
use std::sync::OnceLock;

const OPENRPC_SCHEMA: &str = include_str!("../schemas/openrpc-1.3.json");
const JSON_SCHEMA_TOOLS_META: &str = include_str!("../schemas/json-schema-tools-meta.json");
const OPENAPI3_SCHEMA: &str = include_str!("../schemas/openapi-3.0.json");

/// The URI the OpenRPC meta-schema uses to refer to its schema-object definition.
const JSON_SCHEMA_TOOLS_URI: &str = "https://meta.json-schema.tools";

fn parse(source: &str, name: &str) -> Value {
    serde_json::from_str(source).unwrap_or_else(|e| panic!("vendored {name} is not JSON: {e}"))
}

/// Serves the vendored schemas and refuses every other URI, so nothing is fetched.
struct VendoredOnly;

impl Retrieve for VendoredOnly {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        if uri.as_str().trim_end_matches('/') == JSON_SCHEMA_TOOLS_URI {
            Ok(parse(
                JSON_SCHEMA_TOOLS_META,
                "json-schema-tools meta-schema",
            ))
        } else {
            Err(format!("{uri} is not vendored").into())
        }
    }
}

fn openrpc_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        jsonschema::options()
            .with_draft(Draft::Draft7)
            .with_retriever(VendoredOnly)
            .build(&parse(OPENRPC_SCHEMA, "OpenRPC meta-schema"))
            .expect("the vendored OpenRPC meta-schema compiles")
    })
}

fn openapi3_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        jsonschema::options()
            .with_draft(Draft::Draft4)
            .with_retriever(VendoredOnly)
            .build(&parse(OPENAPI3_SCHEMA, "OpenAPI 3.0 schema"))
            .expect("the vendored OpenAPI 3.0 schema compiles")
    })
}

fn violations(validator: &Validator, doc: &Value) -> Vec<String> {
    validator
        .iter_errors(doc)
        .map(|error| format!("{}: {}", error.instance_path(), error))
        .collect()
}

/// Lists every way `doc` fails the OpenRPC 1.3 meta-schema.
pub fn openrpc_violations(doc: &Value) -> Vec<String> {
    violations(openrpc_validator(), doc)
}

/// Lists every way `doc` fails the OpenAPI 3.0 schema (2021-09-28).
pub fn openapi3_violations(doc: &Value) -> Vec<String> {
    violations(openapi3_validator(), doc)
}

/// Resolves a local reference such as `#/components/schemas/User` against `doc`.
///
/// Returns `None` for a reference that is not local or does not resolve.
pub fn resolve_ref<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    doc.pointer(pointer)
}

/// Lists every `$ref` in `doc` that is not local or does not resolve, with where it was found.
pub fn dangling_refs(doc: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    collect_dangling(doc, doc, String::new(), &mut problems);
    problems
}

fn collect_dangling(doc: &Value, value: &Value, path: String, problems: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let child_path = format!("{path}/{key}");
                if key == "$ref" {
                    match child.as_str() {
                        Some(reference) if resolve_ref(doc, reference).is_some() => {}
                        Some(reference) => problems.push(format!("{child_path}: {reference}")),
                        None => problems.push(format!("{child_path}: not a string")),
                    }
                } else {
                    collect_dangling(doc, child, child_path, problems);
                }
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_dangling(doc, child, format!("{path}/{index}"), problems);
            }
        }
        _ => {}
    }
}

/// Lists every way `value` fails `schema`, a schema from an OpenRPC document
/// whose `$ref`s point into `doc`'s components. OpenRPC schemas are JSON Schema
/// draft 7.
pub fn openrpc_value_violations(doc: &Value, schema: &Value, value: &Value) -> Vec<String> {
    let root = serde_json::json!({
        "allOf": [schema],
        "components": doc.get("components").cloned().unwrap_or(Value::Null),
    });
    match jsonschema::options()
        .with_draft(Draft::Draft7)
        .with_retriever(VendoredOnly)
        .build(&root)
    {
        Ok(validator) => violations(&validator, value),
        Err(error) => vec![format!("schema does not compile: {error}")],
    }
}

/// OpenAPI 3.0 Schema Object keywords that have no draft-7 meaning and that
/// the translation below does not handle. A schema using one fails the check
/// instead of being validated against something weaker.
const UNSUPPORTED_OPENAPI_KEYWORDS: &[&str] = &["discriminator", "xml"];

/// Rewrites an OpenAPI 3.0 schema into JSON Schema draft 7: `nullable: true`
/// becomes a `null` alternative. Only schema positions are visited.
fn openapi_to_draft7(schema: &Value, problems: &mut Vec<String>, path: &str) -> Value {
    let Value::Object(object) = schema else {
        return schema.clone();
    };
    let mut translated = serde_json::Map::new();
    for (key, value) in object {
        if UNSUPPORTED_OPENAPI_KEYWORDS.contains(&key.as_str()) {
            problems.push(format!(
                "{path}: `{key}` is not supported by the value check"
            ));
        }
        let child = format!("{path}/{key}");
        let value = match key.as_str() {
            "properties" | "patternProperties" => Value::Object(
                value
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(name, sub)| (name.clone(), openapi_to_draft7(sub, problems, &child)))
                    .collect(),
            ),
            "allOf" | "anyOf" | "oneOf" => Value::Array(
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|sub| openapi_to_draft7(sub, problems, &child))
                    .collect(),
            ),
            "items" | "additionalProperties" | "not" => openapi_to_draft7(value, problems, &child),
            "nullable" => continue,
            _ => value.clone(),
        };
        translated.insert(key.clone(), value);
    }
    let translated = Value::Object(translated);
    if object.get("nullable") == Some(&Value::Bool(true)) {
        match translated.get("type").and_then(Value::as_str) {
            Some(single) if translated.get("enum").is_none() => {
                let mut with_null = translated.clone();
                with_null["type"] = serde_json::json!([single, "null"]);
                with_null
            }
            _ => serde_json::json!({ "anyOf": [translated, { "type": "null" }] }),
        }
    } else {
        translated
    }
}

/// Lists every way `value` fails `schema`, a schema from an OpenAPI 3.0
/// document whose `$ref`s point into `doc`'s components.
///
/// The schema and every component are translated to draft 7 first; see
/// `openapi_to_draft7` for what is supported.
pub fn openapi_value_violations(doc: &Value, schema: &Value, value: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let schemas: serde_json::Map<String, Value> = doc["components"]["schemas"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, component)| {
            let path = format!("#/components/schemas/{name}");
            (
                name.clone(),
                openapi_to_draft7(component, &mut problems, &path),
            )
        })
        .collect();
    let root = serde_json::json!({
        "allOf": [openapi_to_draft7(schema, &mut problems, "#")],
        "components": { "schemas": schemas },
    });
    if !problems.is_empty() {
        return problems;
    }
    match jsonschema::options()
        .with_draft(Draft::Draft7)
        .with_retriever(VendoredOnly)
        .build(&root)
    {
        Ok(validator) => violations(&validator, value),
        Err(error) => vec![format!("schema does not compile: {error}")],
    }
}

fn assert_clean(kind: &str, problems: Vec<String>, doc: &Value) {
    assert!(
        problems.is_empty(),
        "{kind}:\n  {}\n\ndocument:\n{}",
        problems.join("\n  "),
        serde_json::to_string_pretty(doc).unwrap_or_default()
    );
}

/// Panics, listing every problem, unless `doc` is a valid OpenRPC 1.3 document
/// whose local references all resolve.
pub fn assert_openrpc_conforms(doc: &Value) {
    assert_clean(
        "OpenRPC meta-schema violations",
        openrpc_violations(doc),
        doc,
    );
    assert_clean("dangling $ref", dangling_refs(doc), doc);
}

/// Panics, listing every problem, unless `doc` is a valid OpenAPI 3.0 document
/// whose local references all resolve.
pub fn assert_openapi3_conforms(doc: &Value) {
    assert_clean(
        "OpenAPI 3.0 schema violations",
        openapi3_violations(doc),
        doc,
    );
    assert_clean("dangling $ref", dangling_refs(doc), doc);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_openrpc_document_conforms() {
        let doc = json!({
            "openrpc": "1.3.2",
            "info": { "title": "t", "version": "1" },
            "methods": [{
                "name": "ping",
                "params": [],
                "result": { "name": "result", "schema": { "$ref": "#/components/schemas/Pong" } }
            }],
            "components": { "schemas": { "Pong": { "type": "string" } } }
        });
        assert_openrpc_conforms(&doc);
    }

    #[test]
    fn openrpc_schema_objects_are_checked() {
        let doc = json!({
            "openrpc": "1.3.2",
            "info": { "title": "t", "version": "1" },
            "methods": [],
            "components": { "schemas": { "Bad": { "type": "nonsense" } } }
        });
        assert!(!openrpc_violations(&doc).is_empty());
    }

    #[test]
    fn openapi_rejects_json_schema_only_keywords() {
        let doc = json!({
            "openapi": "3.0.3",
            "info": { "title": "t", "version": "1" },
            "paths": {},
            "components": { "schemas": { "Bad": { "type": "string", "examples": ["a"] } } }
        });
        assert!(!openapi3_violations(&doc).is_empty());
    }

    #[test]
    fn dangling_refs_are_reported_with_their_location() {
        let doc = json!({ "a": { "$ref": "#/missing" }, "b": { "$ref": "#/a" } });
        assert_eq!(dangling_refs(&doc), vec!["/a/$ref: #/missing".to_string()]);
    }

    #[test]
    fn openapi_values_respect_nullable() {
        let doc = json!({ "components": { "schemas": {
            "Item": { "type": "object", "properties": { "nick": { "type": "string", "nullable": true } } }
        } } });
        let item = json!({ "$ref": "#/components/schemas/Item" });
        assert!(openapi_value_violations(&doc, &item, &json!({ "nick": null })).is_empty());
        assert!(!openapi_value_violations(&doc, &item, &json!({ "nick": 3 })).is_empty());
        let optional_item = json!({ "allOf": [item], "nullable": true });
        assert!(openapi_value_violations(&doc, &optional_item, &json!(null)).is_empty());
        let unsupported = json!({ "discriminator": { "propertyName": "kind" } });
        assert!(!openapi_value_violations(&doc, &unsupported, &json!({})).is_empty());
    }
}
