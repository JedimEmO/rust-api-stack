use crate::ServiceDefinition;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use std::collections::HashMap;

pub(super) fn collect_types(service_def: &ServiceDefinition) -> HashMap<String, TokenStream> {
    let mut unique_types = std::collections::HashMap::new();
    for endpoint in &service_def.endpoints {
        if let Some(request_type) = &endpoint.request_type {
            let request_type_str = quote!(#request_type).to_string();
            unique_types.insert(request_type_str, quote!(#request_type));
        }

        let response_type = &endpoint.response_type;
        let response_type_str = quote!(#response_type).to_string();
        unique_types.insert(response_type_str, quote!(#response_type));

        for path_param in &endpoint.path_params {
            let param_type = &path_param.param_type;
            let param_type_str = quote!(#param_type).to_string();
            unique_types.insert(param_type_str, quote!(#param_type));
        }

        for query_param in &endpoint.query_params {
            let param_type = &query_param.param_type;
            let param_type_str = quote!(#param_type).to_string();
            unique_types.insert(param_type_str, quote!(#param_type));
        }

        for version in &endpoint.versions {
            if let Some(request_type) = &version.request_type {
                let request_type_str = quote!(#request_type).to_string();
                unique_types.insert(request_type_str, quote!(#request_type));
            }

            let response_type = &version.response_type;
            let response_type_str = quote!(#response_type).to_string();
            unique_types.insert(response_type_str, quote!(#response_type));

            for path_param in &version.path_params {
                let param_type = &path_param.param_type;
                let param_type_str = quote!(#param_type).to_string();
                unique_types.insert(param_type_str, quote!(#param_type));
            }

            for query_param in &version.query_params {
                let param_type = &query_param.param_type;
                let param_type_str = quote!(#param_type).to_string();
                unique_types.insert(param_type_str, quote!(#param_type));
            }
        }
    }

    unique_types
}

pub(super) fn sanitize_type_name(type_name: &str) -> String {
    if type_name == "()" {
        "Unit".to_string()
    } else {
        type_name
            .replace("::", "_")
            .replace("<", "_")
            .replace(">", "")
            .replace(" ", "")
            .replace(",", "_")
            .replace("(", "_")
            .replace(")", "_")
    }
}

pub(super) fn generate_schemas(
    service_name: &Ident,
    unique_types: &HashMap<String, TokenStream>,
) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let schema_fns: Vec<TokenStream> = unique_types
        .iter()
        .map(|(type_name, type_tokens)| {
            if type_name == "()" {
                quote! {} // Skip unit type, we'll handle it separately
            } else {
                let sanitized_name = sanitize_type_name(type_name);
                let fn_name = quote::format_ident!(
                    "_generate_schema_for_{}_{}",
                    service_name.to_string().to_lowercase(),
                    sanitized_name
                );
                quote! {
                    fn #fn_name() -> serde_json::Value {
                        let schema = schemars::schema_for!(#type_tokens);
                        let mut schema_value = serde_json::to_value(&schema).unwrap_or_else(|_| {
                            serde_json::json!({
                                "type": "object",
                                "description": format!("Schema for {}", #type_name)
                            })
                        });

                        // Post-process schemas for broad OpenAPI explorer compatibility.
                        normalize_nullable_properties(&mut schema_value);
                        fix_option_types(&mut schema_value);
                        schema_value
                    }
                }
            }
        })
        .collect();

    let schema_insertions: Vec<TokenStream> = unique_types
        .keys()
        .map(|type_name| {
            if type_name == "()" {
                quote! {
                    schemas.insert("Unit".to_string(), serde_json::json!({
                        "type": "null",
                        "description": "Unit type (empty response)"
                    }));
                }
            } else {
                let sanitized_name = sanitize_type_name(type_name);
                let fn_name = quote::format_ident!(
                    "_generate_schema_for_{}_{}",
                    service_name.to_string().to_lowercase(),
                    sanitized_name
                );
                quote! {
                    schemas.insert(#sanitized_name.to_string(), #fn_name());
                }
            }
        })
        .collect();

    (schema_fns, schema_insertions)
}

pub(super) fn generate_normalization() -> TokenStream {
    quote! {
        // Helper function to fix schema references and flatten nested definitions
        fn fix_schema_refs(value: &mut serde_json::Value, schemas: &mut serde_json::Map<String, serde_json::Value>) {
            match value {
                serde_json::Value::Object(obj) => {
                    if let Some(defs) = obj.remove("definitions") {
                        if let serde_json::Value::Object(defs_obj) = defs {
                            for (name, schema) in defs_obj {
                                let mut schema_copy = schema.clone();
                                fix_schema_refs(&mut schema_copy, schemas);
                                schemas.insert(name, schema_copy);
                            }
                        }
                    }

                    if let Some(defs) = obj.remove("$defs") {
                        if let serde_json::Value::Object(defs_obj) = defs {
                            for (name, schema) in defs_obj {
                                let mut schema_copy = schema.clone();
                                fix_schema_refs(&mut schema_copy, schemas);
                                schemas.insert(name, schema_copy);
                            }
                        }
                    }

                    if let Some(ref_val) = obj.get_mut("$ref") {
                        if let serde_json::Value::String(ref_str) = ref_val {
                            if ref_str.starts_with("#/definitions/") {
                                let name = ref_str.trim_start_matches("#/definitions/");
                                *ref_str = format!("#/components/schemas/{}", name);
                            } else if ref_str.starts_with("#/$defs/") {
                                let name = ref_str.trim_start_matches("#/$defs/");
                                *ref_str = format!("#/components/schemas/{}", name);
                            }
                        }
                    }

                    // Remove $schema field as it's not needed in OpenAPI
                    obj.remove("$schema");

                    for (_, v) in obj.iter_mut() {
                        fix_schema_refs(v, schemas);
                    }
                }
                serde_json::Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        fix_schema_refs(item, schemas);
                    }
                }
                _ => {}
            }
        }

        // Helper function to normalize nullable properties for better OpenAPI explorer compatibility.
        fn normalize_nullable_properties(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(obj) => {
                    if let Some(properties) = obj.get_mut("properties") {
                        if let serde_json::Value::Object(props) = properties {
                            for (_, prop_value) in props.iter_mut() {
                                if let serde_json::Value::Object(prop_obj) = prop_value {
                                    if let Some(type_val) = prop_obj.get("type") {
                                        if let serde_json::Value::Array(type_array) = type_val {
                                            if type_array.len() == 2 {
                                                let null_value = serde_json::Value::String("null".to_string());
                                                if type_array.contains(&null_value) {
                                                    let non_null_type = type_array.iter()
                                                        .find(|t| **t != null_value)
                                                        .cloned();

                                                    if let Some(actual_type) = non_null_type {
                                                        prop_obj.insert("type".to_string(), actual_type);
                                                        prop_obj.insert("nullable".to_string(), serde_json::Value::Bool(true));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                normalize_nullable_properties(prop_value);
                            }
                        }
                    }

                    if let Some(definitions) = obj.get_mut("definitions") {
                        normalize_nullable_properties(definitions);
                    }

                    for (_, v) in obj.iter_mut() {
                        normalize_nullable_properties(v);
                    }
                }
                serde_json::Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        normalize_nullable_properties(item);
                    }
                }
                _ => {}
            }
        }

        // Helper function to fix Option types that use anyOf with null or type arrays
        fn fix_option_types(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(obj) => {
                    if let Some(type_val) = obj.get("type") {
                        if let serde_json::Value::Array(type_array) = type_val {
                            if type_array.len() == 2 {
                                let null_value = serde_json::Value::String("null".to_string());
                                if type_array.contains(&null_value) {
                                    let non_null_type = type_array.iter()
                                        .find(|t| **t != null_value)
                                        .cloned();

                                    if let Some(actual_type) = non_null_type {
                                        obj.insert("type".to_string(), actual_type);
                                        obj.insert("nullable".to_string(), serde_json::Value::Bool(true));
                                    }
                                }
                            }
                        }
                    }

                    if let Some(any_of) = obj.get_mut("anyOf") {
                        if let serde_json::Value::Array(any_of_array) = any_of {
                            if any_of_array.len() == 2 {
                                let has_null = any_of_array.iter().any(|item| {
                                    if let serde_json::Value::Object(item_obj) = item {
                                        if let Some(type_val) = item_obj.get("type") {
                                            if let serde_json::Value::String(type_str) = type_val {
                                                return type_str == "null";
                                            }
                                        }
                                    }
                                    false
                                });

                                if has_null {
                                    let non_null_schema = any_of_array.iter().find(|item| {
                                        if let serde_json::Value::Object(item_obj) = item {
                                            if let Some(type_val) = item_obj.get("type") {
                                                if let serde_json::Value::String(type_str) = type_val {
                                                    return type_str != "null";
                                                }
                                            }
                                            // If it has other properties besides type, it's not the null schema
                                            return item_obj.len() > 1 || !item_obj.contains_key("type");
                                        }
                                        true
                                    }).cloned();

                                    if let Some(schema) = non_null_schema {
                                        obj.remove("anyOf");
                                        if let serde_json::Value::Object(schema_obj) = schema {
                                            for (key, val) in schema_obj {
                                                obj.insert(key, val);
                                            }
                                        }
                                        obj.insert("nullable".to_string(), serde_json::Value::Bool(true));
                                    }
                                }
                            }
                        }
                    }

                    for (_, v) in obj.iter_mut() {
                        fix_option_types(v);
                    }
                }
                serde_json::Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        fix_option_types(item);
                    }
                }
                _ => {}
            }
        }

    }
}

/// Generates code to include schema generation for types when schemars is available
pub fn generate_schema_impl_checks(service_def: &ServiceDefinition) -> TokenStream {
    let mut unique_types = HashMap::new();

    for endpoint in &service_def.endpoints {
        if let Some(request_type) = &endpoint.request_type {
            unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
        }

        let response_type = &endpoint.response_type;
        unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

        for path_param in &endpoint.path_params {
            let param_type = &path_param.param_type;
            unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
        }

        for query_param in &endpoint.query_params {
            let param_type = &query_param.param_type;
            unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
        }

        for version in &endpoint.versions {
            if let Some(request_type) = &version.request_type {
                unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
            }

            let response_type = &version.response_type;
            unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

            for path_param in &version.path_params {
                let param_type = &path_param.param_type;
                unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
            }

            for query_param in &version.query_params {
                let param_type = &query_param.param_type;
                unique_types.insert(quote!(#param_type).to_string(), quote!(#param_type));
            }
        }
    }

    let type_checks: Vec<TokenStream> = unique_types
        .values()
        .map(|type_tokens| {
            quote! {
                const _: () = {
                    fn _assert_json_schema<T: schemars::JsonSchema>() {}
                    fn _check() {
                        _assert_json_schema::<#type_tokens>();
                    }
                };
            }
        })
        .collect();

    quote! {
        #(#type_checks)*
    }
}
