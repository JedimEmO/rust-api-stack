//! Builds the `components.schemas` map of a generated API document.
//!
//! This file is embedded verbatim in macro-generated code, so it may only use
//! `std`, `schemars`, `serde` and `serde_json`, always through absolute paths,
//! and only syntax every edition accepts (no let chains).

use ::schemars::transform::{transform_subschemas, Transform};
use ::schemars::{JsonSchema, Schema, SchemaGenerator};
use ::serde_json::{Map, Value};
use ::std::collections::BTreeSet;
// In the prelude only from edition 2021; this file compiles in older editions too.
use ::std::convert::TryFrom;

/// Where component schemas live; every generated reference starts with this.
pub const COMPONENTS_PREFIX: &str = "#/components/schemas/";

/// Appended to the name of a type whose serialized form differs from what it accepts.
pub const SERIALIZE_SUFFIX: &str = ".Serialize";

/// The schema dialect of a document.
#[derive(Clone, Copy)]
pub enum Dialect {
    /// JSON Schema draft 7, as OpenRPC uses.
    OpenRpc,
    /// The OpenAPI 3.0 Schema Object.
    OpenApi3,
}

/// One generator per contract, sharing a dialect.
pub struct Generators {
    /// For what the server reads: requests, parameters.
    pub deserialize: SchemaGenerator,
    /// For what the server sends: responses.
    pub serialize: SchemaGenerator,
}

impl Generators {
    /// Creates both generators, with references pointing at [`COMPONENTS_PREFIX`].
    pub fn new(dialect: Dialect) -> Self {
        let settings = match dialect {
            Dialect::OpenRpc => ::schemars::generate::SchemaSettings::draft07(),
            Dialect::OpenApi3 => ::schemars::generate::SchemaSettings::openapi3(),
        }
        .with(|settings| {
            settings.definitions_path = "/components/schemas".into();
            settings.meta_schema = None;
        });
        Self {
            deserialize: settings.clone().for_deserialize().into_generator(),
            serialize: settings.for_serialize().into_generator(),
        }
    }

    /// The schema for `T` as the server reads it.
    pub fn request<T: ?Sized + JsonSchema>(&mut self) -> Value {
        subschema::<T>(&mut self.deserialize)
    }

    /// The schema for `T` as the server sends it.
    ///
    /// Pass it through [`Components::fix_serialized`] once all schemas are generated.
    pub fn response<T: ?Sized + JsonSchema>(&mut self) -> Value {
        subschema::<T>(&mut self.serialize)
    }

    /// Merges the definitions both generators collected.
    pub fn into_components(mut self) -> Result<Components, String> {
        Components::merge(
            self.deserialize.take_definitions(true),
            self.serialize.take_definitions(true),
        )
    }
}

/// Generates `T` as a subschema, with the generator's dialect transforms applied.
fn subschema<T: ?Sized + JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    let mut schema = generator.subschema_for::<T>();
    for transform in generator.transforms_mut() {
        transform.transform(&mut schema);
    }
    schema.to_value()
}

/// The merged `components.schemas` map.
pub struct Components {
    /// Component schemas by name.
    pub schemas: Map<String, Value>,
    /// Names whose serialization schema lives under `<name>.Serialize`.
    renamed: BTreeSet<String>,
}

impl Components {
    /// Merges deserialization and serialization definitions.
    ///
    /// Fails when a `<name>.Serialize` key it needs is already taken.
    pub fn merge(
        deserialize: Map<String, Value>,
        serialize: Map<String, Value>,
    ) -> Result<Self, String> {
        // A schema that only refers to a renamed type differs too,
        // so repeat until no new name turns out to differ.
        let mut renamed = BTreeSet::new();
        loop {
            let mut changed = false;
            for (name, serialized) in &serialize {
                if renamed.contains(name) {
                    continue;
                }
                if let Some(deserialized) = deserialize.get(name) {
                    let mut candidate = serialized.clone();
                    rename_refs(&mut candidate, &renamed);
                    if candidate != *deserialized {
                        renamed.insert(name.clone());
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        for name in &renamed {
            let key = format!("{name}{SERIALIZE_SUFFIX}");
            if deserialize.contains_key(&key) || serialize.contains_key(&key) {
                return Err(format!(
                    "component `{key}` is needed for the serialized form of `{name}`, \
                     but another type already uses that name"
                ));
            }
        }

        let mut schemas = deserialize;
        for (name, mut schema) in serialize {
            rename_refs(&mut schema, &renamed);
            if renamed.contains(&name) {
                schemas.insert(format!("{name}{SERIALIZE_SUFFIX}"), schema);
            } else if !schemas.contains_key(&name) {
                schemas.insert(name, schema);
            }
        }

        if let Some(name) = schemas.keys().find(|name| !is_valid_component_key(name)) {
            return Err(format!(
                "component name `{name}` may only use letters, digits, `.`, `-` and `_`; \
                 rename the type's schema"
            ));
        }

        Ok(Self { schemas, renamed })
    }

    /// Points references in a schema from [`Generators::response`] at the right components.
    pub fn fix_serialized(&self, schema: &mut Value) {
        rename_refs(schema, &self.renamed);
    }
}

/// Whether `name` matches the component key pattern both formats accept,
/// `^[a-zA-Z0-9.\-_]+$`.
pub fn is_valid_component_key(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// Appends [`SERIALIZE_SUFFIX`] to references naming a type in `renamed`.
///
/// Visits schema positions only, so example and default values are never touched.
fn rename_refs(value: &mut Value, renamed: &BTreeSet<String>) {
    struct Rename<'a>(&'a BTreeSet<String>);

    impl Transform for Rename<'_> {
        fn transform(&mut self, schema: &mut Schema) {
            if let Some(Value::String(reference)) = schema.get_mut("$ref") {
                let target = reference
                    .strip_prefix(COMPONENTS_PREFIX)
                    .map(decode_ref_name);
                if target.is_some_and(|name| self.0.contains(&name)) {
                    reference.push_str(SERIALIZE_SUFFIX);
                }
            }
            transform_subschemas(self, schema);
        }
    }

    if renamed.is_empty() {
        return;
    }
    if let Ok(schema) = <&mut Schema>::try_from(value) {
        Rename(renamed).transform(schema);
    }
}

/// Decodes a reference's last segment back into a definition name
/// (JSON Pointer escapes and URI percent-encoding).
fn decode_ref_name(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |offset: usize| {
            bytes
                .get(index + offset)
                .and_then(|byte| (*byte as char).to_digit(16))
        };
        match (bytes[index], hex(1), hex(2)) {
            (b'%', Some(high), Some(low)) => {
                decoded.push((high * 16 + low) as u8);
                index += 3;
            }
            (byte, _, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded)
        .replace("~1", "/")
        .replace("~0", "~")
}

/// How a JSON-RPC request's `params` can be described.
pub enum ParamShape {
    /// An object with a fixed set of fields: one descriptor per field, called by name.
    Named(Vec<NamedParam>),
    /// Anything else: one descriptor for the whole `params` value.
    Raw,
}

/// One field of a [`ParamShape::Named`] request.
pub struct NamedParam {
    /// The field's name on the wire.
    pub name: String,
    /// Whether the request schema lists the field as required.
    pub required: bool,
    /// The field's schema.
    pub schema: Value,
}

/// Keys that only annotate a schema and never constrain values.
const ANNOTATION_KEYS: &[&str] = &[
    "title",
    "description",
    "default",
    "example",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "nullable",
    "$comment",
];

/// Keys an object schema may carry and still be described by named descriptors.
/// `additionalProperties` is checked separately: only `false` is allowed.
const NAMED_OBJECT_KEYS: &[&str] = &["type", "properties", "required", "additionalProperties"];

/// The reference inside `{ "allOf": [{ "$ref": ... }], <annotations> }`, the form
/// schemars uses for a reference that carries a description.
pub fn wrapped_ref(schema: &Value) -> Option<&str> {
    let object = schema.as_object()?;
    let [inner] = object.get("allOf")?.as_array()?.as_slice() else {
        return None;
    };
    let reference = inner
        .as_object()
        .filter(|o| o.len() == 1)?
        .get("$ref")?
        .as_str()?;
    let annotations_only = object
        .keys()
        .all(|key| key == "allOf" || ANNOTATION_KEYS.contains(&key.as_str()));
    annotations_only.then_some(reference)
}

/// Follows component references and reference wrappers to the schema they stand for.
///
/// Returns `None` for a reference that leaves the document, does not resolve, or loops.
fn resolve<'a>(mut schema: &'a Value, components: &'a Map<String, Value>) -> Option<&'a Value> {
    let mut visited = BTreeSet::new();
    loop {
        let reference = match schema.get("$ref").and_then(Value::as_str) {
            Some(reference) if schema.as_object().is_some_and(|o| o.len() == 1) => reference,
            Some(_) => return None,
            None => match wrapped_ref(schema) {
                Some(reference) => reference,
                None => return Some(schema),
            },
        };
        let name = decode_ref_name(reference.strip_prefix(COMPONENTS_PREFIX)?);
        if !visited.insert(name.clone()) {
            return None;
        }
        schema = components.get(&name)?;
    }
}

/// Decides whether a request can be called by name, and with which fields.
///
/// A request is named when its schema resolves to an object with a fixed set of
/// properties and nothing else the descriptors could not carry. Anything not
/// recognised is raw, never simplified.
pub fn classify_params(schema: &Value, components: &Map<String, Value>) -> ParamShape {
    let Some(object) = resolve(schema, components).and_then(Value::as_object) else {
        return ParamShape::Raw;
    };
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return ParamShape::Raw;
    }
    let known = |key: &String| {
        NAMED_OBJECT_KEYS.contains(&key.as_str()) || ANNOTATION_KEYS.contains(&key.as_str())
    };
    if !object.keys().all(known) {
        return ParamShape::Raw;
    }
    if object
        .get("additionalProperties")
        .is_some_and(|value| value != &Value::Bool(false))
    {
        return ParamShape::Raw;
    }
    // A struct without fields may have no `properties` at all.
    let empty = Map::new();
    let properties = match object.get("properties") {
        None => &empty,
        Some(Value::Object(properties)) => properties,
        Some(_) => return ParamShape::Raw,
    };
    let required: BTreeSet<&str> = match object.get("required") {
        None => BTreeSet::new(),
        Some(Value::Array(names)) => match names.iter().map(Value::as_str).collect() {
            Some(names) => names,
            None => return ParamShape::Raw,
        },
        Some(_) => return ParamShape::Raw,
    };
    if !required.iter().all(|name| properties.contains_key(*name)) {
        return ParamShape::Raw;
    }
    ParamShape::Named(
        properties
            .iter()
            .map(|(name, schema)| NamedParam {
                name: name.clone(),
                required: required.contains(name.as_str()),
                schema: schema.clone(),
            })
            .collect(),
    )
}

/// An authored example, evaluated to JSON.
pub struct Example {
    /// The example's identifier.
    pub name: String,
    /// The example's doc comment, first line included.
    pub description: Option<String>,
    /// The serialized request; `None` for a method whose request is `()`.
    pub params: Option<Value>,
    /// What the call returns.
    pub outcome: ExampleOutcome,
}

/// What an [`Example`] call returns.
pub enum ExampleOutcome {
    /// The serialized result.
    Result(Value),
    /// A JSON-RPC error object.
    Error {
        /// The error code.
        code: i64,
        /// The error message.
        message: String,
        /// The serialized `data`, if any.
        data: Option<Value>,
    },
}

/// Serializes an example value.
pub fn to_json<T: ::serde::Serialize + ?Sized>(value: &T) -> Result<Value, String> {
    ::serde_json::to_value(value).map_err(|error| format!("does not serialize: {error}"))
}

/// Serializes a request example and checks that the request type reads it back unchanged.
///
/// A `#[serde(default, rename(serialize = "out", deserialize = "in"))]` field
/// decodes without error but loses its value, so decoding alone is not enough:
/// the decoded value must serialize to the same JSON.
pub fn round_trip<T>(value: &T) -> Result<Value, String>
where
    T: ::serde::Serialize + ::serde::de::DeserializeOwned,
{
    let written = to_json(value)?;
    let decoded: T = ::serde_json::from_value(written.clone())
        .map_err(|_| "does not deserialize back into the request type".to_string())?;
    if to_json(&decoded)? != written {
        return Err(
            "reads back differently than it was written; check serde renames, \
                    defaults and skipped fields on the request type"
                .to_string(),
        );
    }
    Ok(written)
}

/// The `params` list of an example pairing: one entry per serialized field for a
/// named method, the whole value for a raw one, nothing for a `()` request.
fn pairing_params(shape: Option<&ParamShape>, params: Option<Value>) -> Result<Vec<Value>, String> {
    match (shape, params) {
        (None, _) | (_, None) => Ok(Vec::new()),
        (Some(ParamShape::Raw), Some(params)) => Ok(vec![
            ::serde_json::json!({ "name": "params", "value": params }),
        ]),
        (Some(ParamShape::Named(fields)), Some(params)) => {
            let Value::Object(params) = params else {
                return Err("params do not serialize to an object".to_string());
            };
            params
                .into_iter()
                .map(|(name, value)| {
                    if fields.iter().any(|field| field.name == name) {
                        Ok(::serde_json::json!({ "name": name, "value": value }))
                    } else {
                        Err(format!("params field `{name}` has no parameter descriptor"))
                    }
                })
                .collect()
        }
    }
}

/// Builds a method's OpenRPC example pairings and its `x-ras-error-examples`.
///
/// Fails with the example's name and the cause when a request field has no
/// descriptor.
pub fn example_objects(
    shape: Option<&ParamShape>,
    examples: Vec<Example>,
) -> Result<(Vec<Value>, Vec<Value>), (String, String)> {
    let mut pairings = Vec::new();
    let mut error_examples = Vec::new();
    for example in examples {
        let params =
            pairing_params(shape, example.params).map_err(|cause| (example.name.clone(), cause))?;
        let mut object = Map::new();
        object.insert("name".into(), Value::String(example.name));
        if let Some(description) = example.description {
            object.insert("description".into(), Value::String(description));
        }
        object.insert("params".into(), Value::Array(params));
        match example.outcome {
            ExampleOutcome::Result(value) => {
                object.insert(
                    "result".into(),
                    ::serde_json::json!({ "name": "result", "value": value }),
                );
                pairings.push(Value::Object(object));
            }
            ExampleOutcome::Error {
                code,
                message,
                data,
            } => {
                let mut error = ::serde_json::json!({ "code": code, "message": message });
                if let Some(data) = data {
                    error["data"] = data;
                }
                object.insert("error".into(), error);
                error_examples.push(Value::Object(object));
            }
        }
    }
    Ok((pairings, error_examples))
}

/// A method's OpenRPC `errors` list: the built-in errors it can produce, as
/// references into `components.errors`, then each application code its error
/// examples use, once, with the first example's message.
pub fn method_errors(has_params: bool, auth_required: bool, examples: &[Example]) -> Vec<Value> {
    let reference =
        |name: &str| ::serde_json::json!({ "$ref": format!("#/components/errors/{name}") });
    let documents_invalid_params = examples
        .iter()
        .any(|example| matches!(example.outcome, ExampleOutcome::Error { code: -32602, .. }));
    let mut errors = Vec::new();
    if has_params || documents_invalid_params {
        errors.push(reference("InvalidParams"));
    }
    if auth_required {
        errors.push(reference("AuthenticationRequired"));
        errors.push(reference("InsufficientPermissions"));
        errors.push(reference("TokenExpired"));
    }
    errors.push(reference("InternalError"));

    let mut seen = BTreeSet::new();
    for example in examples {
        if let ExampleOutcome::Error { code, message, .. } = &example.outcome {
            // -32602 is the built-in InvalidParams entry.
            if *code == -32602 || !seen.insert(*code) {
                continue;
            }
            errors.push(::serde_json::json!({ "code": code, "message": message }));
        }
    }
    errors
}

/// The component key of the body a REST handler error is sent with.
pub const REST_ERROR_BODY: &str = "RestErrorBody";

/// The schema of [`REST_ERROR_BODY`]: `{ "error": "<message>" }`.
pub fn rest_error_body_schema() -> Value {
    ::serde_json::json!({
        "type": "object",
        "description": "The body of every error response: a handler's `RestError` or a request the framework rejects.",
        "properties": { "error": { "type": "string" } },
        "required": ["error"]
    })
}

/// A reference to [`REST_ERROR_BODY`].
pub fn rest_error_body_ref() -> Value {
    ::serde_json::json!({ "$ref": format!("{COMPONENTS_PREFIX}{REST_ERROR_BODY}") })
}

/// Describes every error response an operation lists with [`REST_ERROR_BODY`].
///
/// Every REST error, from a handler or from the framework, is sent as
/// `{ "error": "<message>" }`, so the schema holds whether or not an example
/// uses the status.
pub fn describe_rest_error_responses(operation: &mut Value) {
    if let Some(responses) = operation["responses"].as_object_mut() {
        for (status, response) in responses.iter_mut() {
            let is_error = status.starts_with('4') || status.starts_with('5');
            if is_error && response.is_object() {
                response["content"]["application/json"]["schema"] = rest_error_body_ref();
            }
        }
    }
}

/// An authored REST example, evaluated to JSON.
pub struct RestExample {
    /// The example's identifier, used as its key everywhere it appears.
    pub name: String,
    /// The first line of the example's doc comment.
    pub summary: Option<String>,
    /// The whole doc comment.
    pub description: Option<String>,
    /// Path parameter values by name.
    pub path: Vec<(String, Value)>,
    /// Query parameter values by name; `null` means left out.
    pub query: Vec<(String, Value)>,
    /// The serialized request body, if the endpoint takes one.
    pub body: Option<Value>,
    /// The response status.
    pub status: u16,
    /// The serialized response body, for a success.
    pub response: Option<Value>,
    /// The error message, for a `RestError`.
    pub error: Option<String>,
}

impl RestExample {
    fn object(&self, value: Value) -> Value {
        let mut object = Map::new();
        if let Some(summary) = &self.summary {
            object.insert("summary".into(), Value::String(summary.clone()));
        }
        if let Some(description) = &self.description {
            object.insert("description".into(), Value::String(description.clone()));
        }
        object.insert("value".into(), value);
        Value::Object(object)
    }
}

/// Inserts `example` under `name` in `target["examples"]`.
fn insert_example(target: &mut Value, name: &str, example: Value) {
    if !target["examples"].is_object() {
        target["examples"] = Value::Object(Map::new());
    }
    target["examples"][name] = example;
}

/// Adds REST examples to an OpenAPI operation.
///
/// OpenAPI 3.0 has no request/response pairs, so each example appears under
/// the same key in every place it touches: its parameters, its request body and
/// the response for its status. A status no response lists yet gets one; an
/// error response is described by [`REST_ERROR_BODY`].
pub fn apply_rest_examples(
    operation: &mut Value,
    response_schema: &Value,
    examples: Vec<RestExample>,
) {
    for example in examples {
        let parameter_values = example
            .path
            .iter()
            .map(|(name, value)| ("path", name, value))
            .chain(
                example
                    .query
                    .iter()
                    .map(|(name, value)| ("query", name, value)),
            );
        for (location, name, value) in parameter_values {
            if value.is_null() {
                continue;
            }
            let object = example.object(value.clone());
            // Nested, not a let chain: this file compiles in the consumer's edition.
            if let Some(parameters) = operation["parameters"].as_array_mut() {
                if let Some(parameter) = parameters
                    .iter_mut()
                    .find(|p| p["in"] == location && p["name"] == name.as_str())
                {
                    insert_example(parameter, &example.name, object);
                }
            }
        }

        if let Some(body) = &example.body {
            if operation["requestBody"].is_object() {
                let object = example.object(body.clone());
                insert_example(
                    &mut operation["requestBody"]["content"]["application/json"],
                    &example.name,
                    object,
                );
            }
        }

        let status = example.status.to_string();
        if !operation["responses"].is_object() {
            operation["responses"] = Value::Object(Map::new());
        }
        let responses = &mut operation["responses"];
        match &example.error {
            Some(message) => {
                let response = &mut responses[status.as_str()];
                if !response.is_object() {
                    *response =
                        ::serde_json::json!({ "description": "Error returned by the handler" });
                }
                let media = &mut response["content"]["application/json"];
                media["schema"] = rest_error_body_ref();
                let object = example.object(::serde_json::json!({ "error": message }));
                insert_example(media, &example.name, object);
            }
            None if example.status == 204 => {
                if !responses["204"].is_object() {
                    responses["204"] = ::serde_json::json!({ "description": "No content" });
                }
                // A 204 has no content to hang the example on; the extension
                // keeps its name and docs with the status.
                let object = example.object(Value::Null);
                if !responses["204"]["x-ras-examples"].is_object() {
                    responses["204"]["x-ras-examples"] = Value::Object(Map::new());
                }
                responses["204"]["x-ras-examples"][example.name.as_str()] = object;
            }
            None => {
                let response = &mut responses[status.as_str()];
                if !response.is_object() {
                    *response = ::serde_json::json!({
                        "description": "Successful response",
                        "content": { "application/json": { "schema": response_schema } }
                    });
                }
                let value = example.response.clone().unwrap_or(Value::Null);
                let object = example.object(value);
                insert_example(
                    &mut response["content"]["application/json"],
                    &example.name,
                    object,
                );
            }
        }
    }
}
