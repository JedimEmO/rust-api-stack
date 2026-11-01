//! Builds the `components.schemas` map of a generated API document.
//!
//! This file is embedded verbatim in macro-generated code, so it may only use
//! `std`, `schemars` and `serde_json`, always through absolute paths.

use ::schemars::transform::{Transform, transform_subschemas};
use ::schemars::{JsonSchema, Schema, SchemaGenerator};
use ::serde_json::{Map, Value};
use ::std::collections::BTreeSet;

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

        Ok(Self { schemas, renamed })
    }

    /// Points references in a schema from [`Generators::response`] at the right components.
    pub fn fix_serialized(&self, schema: &mut Value) {
        rename_refs(schema, &self.renamed);
    }
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
