use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) fn generate_examples(generate_example_fn_name: &Ident) -> TokenStream {
    quote! {
        /// Generate example value from schema
        fn #generate_example_fn_name(schema: &serde_json::Value, schemas: &std::collections::HashMap<String, serde_json::Value>) -> serde_json::Value {
            if let Some(examples) = schema.get("examples") {
                if let Some(arr) = examples.as_array() {
                    if let Some(first) = arr.first() {
                        return first.clone();
                    }
                }
            }

            if let Some(example) = schema.get("example") {
                return example.clone();
            }

            if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
                if let Some(ref_name) = ref_str.strip_prefix("#/components/schemas/") {
                    if let Some(ref_schema) = schemas.get(ref_name) {
                        return #generate_example_fn_name(ref_schema, schemas);
                    }
                }
            }

            // Handle oneOf/anyOf - pick the first variant
            if let Some(one_of) = schema.get("oneOf").and_then(|v| v.as_array()) {
                if let Some(first_variant) = one_of.first() {
                    return #generate_example_fn_name(first_variant, schemas);
                }
            }
            if let Some(any_of) = schema.get("anyOf").and_then(|v| v.as_array()) {
                if let Some(first_variant) = any_of.first() {
                    return #generate_example_fn_name(first_variant, schemas);
                }
            }

            match schema.get("type").and_then(|v| v.as_str()) {
                Some("string") => serde_json::json!("example_string"),
                Some("number") | Some("integer") => serde_json::json!(42),
                Some("boolean") => serde_json::json!(true),
                Some("array") => {
                    if let Some(items) = schema.get("items") {
                        serde_json::json!([#generate_example_fn_name(items, schemas)])
                    } else {
                        serde_json::json!(["example_item"])
                    }
                }
                Some("object") => {
                    let mut obj = serde_json::Map::new();
                    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
                        for (key, prop_schema) in props {
                            obj.insert(key.clone(), #generate_example_fn_name(prop_schema, schemas));
                        }
                        serde_json::json!(obj)
                    } else {
                        serde_json::json!({"example_key": "example_value"})
                    }
                }
                Some("null") => serde_json::json!(null),
                _ => serde_json::json!({"example": "value"})
            }
        }

    }
}
