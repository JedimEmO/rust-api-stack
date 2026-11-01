//! Schema component assembly for generated OpenRPC and OpenAPI documents.
//!
//! The service macros embed [`SOURCE`] in their output as a private module.
//! See the README for why it is source and not a library dependency.

/// The text of `src/components.rs`, for the macros to embed.
pub const SOURCE: &str = include_str!("components.rs");

#[cfg(test)]
#[allow(dead_code)]
#[path = "components.rs"]
mod components;

#[cfg(test)]
mod tests {
    use super::components::{COMPONENTS_PREFIX, Components, Dialect, Generators};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::{Map, Value, json};

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct Renamed {
        #[serde(rename(serialize = "out", deserialize = "in"))]
        value: String,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct Holder {
        inner: Renamed,
        /// Shown as a default; must never be rewritten.
        #[serde(default = "example_ref")]
        note: Value,
    }

    fn example_ref() -> Value {
        json!({ "$ref": "#/components/schemas/Renamed" })
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct Plain {
        id: u32,
    }

    fn schemas_of(components: &Components) -> &Map<String, Value> {
        &components.schemas
    }

    #[test]
    fn equal_contracts_share_one_component() {
        let mut generators = Generators::new(Dialect::OpenRpc);
        assert_eq!(
            generators.request::<Plain>(),
            json!({ "$ref": format!("{COMPONENTS_PREFIX}Plain") })
        );
        let mut response = generators.response::<Plain>();
        let components = generators.into_components().unwrap();
        components.fix_serialized(&mut response);
        assert_eq!(
            response,
            json!({ "$ref": format!("{COMPONENTS_PREFIX}Plain") })
        );
        assert_eq!(schemas_of(&components).len(), 1);
    }

    #[test]
    fn differing_contracts_get_a_serialize_component_and_dependents_follow() {
        let mut generators = Generators::new(Dialect::OpenApi3);
        let request = generators.request::<Holder>();
        let mut response = generators.response::<Holder>();
        let components = generators.into_components().unwrap();
        components.fix_serialized(&mut response);

        let schemas = schemas_of(&components);
        assert!(schemas["Renamed"]["properties"].get("in").is_some());
        assert!(
            schemas["Renamed.Serialize"]["properties"]
                .get("out")
                .is_some()
        );
        // Holder only differs through its reference, and still gets its own component.
        assert_eq!(
            schemas["Holder.Serialize"]["properties"]["inner"]["$ref"],
            "#/components/schemas/Renamed.Serialize"
        );
        assert_eq!(
            schemas["Holder"]["properties"]["inner"]["$ref"],
            "#/components/schemas/Renamed"
        );
        assert_eq!(request["$ref"], "#/components/schemas/Holder");
        assert_eq!(response["$ref"], "#/components/schemas/Holder.Serialize");
        // The default value looks like a reference but is data.
        assert_eq!(
            schemas["Holder.Serialize"]["properties"]["note"]["default"]["$ref"],
            "#/components/schemas/Renamed"
        );
    }

    #[test]
    fn a_taken_serialize_name_is_an_error() {
        let mut deserialize = Map::new();
        deserialize.insert("A".into(), json!({ "type": "string" }));
        deserialize.insert("A.Serialize".into(), json!({ "type": "integer" }));
        let mut serialize = Map::new();
        serialize.insert("A".into(), json!({ "type": "boolean" }));
        let error = Components::merge(deserialize, serialize).err().unwrap();
        assert!(error.contains("A.Serialize"), "{error}");
    }
}
