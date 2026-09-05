use crate::ServiceDefinition;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use std::collections::HashMap;

pub(super) fn collect_types(service_def: &ServiceDefinition) -> HashMap<String, TokenStream> {
    let mut unique_types = std::collections::HashMap::new();
    for method in &service_def.methods {
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        let request_type_str = quote!(#request_type).to_string();
        let response_type_str = quote!(#response_type).to_string();

        unique_types.insert(request_type_str, quote!(#request_type));
        unique_types.insert(response_type_str, quote!(#response_type));

        for version in &method.versions {
            let request_type = &version.request_type;
            let response_type = &version.response_type;
            let request_type_str = quote!(#request_type).to_string();
            let response_type_str = quote!(#response_type).to_string();

            unique_types.insert(request_type_str, quote!(#request_type));
            unique_types.insert(response_type_str, quote!(#response_type));
        }
    }

    unique_types
}

pub(super) fn generate_schemas(
    service_name: &Ident,
    flatten_fn_name: &Ident,
    unique_types: &HashMap<String, TokenStream>,
) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let schema_fns: Vec<TokenStream> = unique_types
        .iter()
        .map(|(type_name, type_tokens)| {
            if type_name == "()" {
                quote! {} // Skip unit type, we'll handle it separately
            } else {
                let fn_name = quote::format_ident!(
                    "_generate_schema_for_{}_{}",
                    service_name.to_string().to_lowercase(),
                    type_name
                        .replace("::", "_")
                        .replace("<", "_")
                        .replace(">", "_")
                        .replace(" ", "_")
                );
                quote! {
                    fn #fn_name() -> (serde_json::Value, std::collections::HashMap<String, serde_json::Value>) {
                        let schema = schemars::schema_for!(#type_tokens);
                        let schema_value = serde_json::to_value(&schema).unwrap_or_else(|_| {
                            serde_json::json!({
                                "type": "object",
                                "description": format!("Schema for {}", #type_name)
                            })
                        });

                        let mut extracted_defs = std::collections::HashMap::new();
                        let flattened_schema = #flatten_fn_name(schema_value, &mut extracted_defs);
                        (flattened_schema, extracted_defs)
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
                    schemas.insert("()".to_string(), serde_json::json!({
                        "type": "null",
                        "description": "Unit type"
                    }));
                }
            } else {
                let fn_name = quote::format_ident!(
                    "_generate_schema_for_{}_{}",
                    service_name.to_string().to_lowercase(),
                    type_name
                        .replace("::", "_")
                        .replace("<", "_")
                        .replace(">", "_")
                        .replace(" ", "_")
                );
                quote! {
                    let (schema, defs) = #fn_name();
                    let sanitized_name = #type_name.to_string().replace(" ", "");
                    schemas.insert(sanitized_name, schema);
                    for (def_name, def_schema) in defs {
                        let sanitized_def_name = def_name.replace(" ", "");
                        schemas.insert(sanitized_def_name, def_schema);
                    }
                }
            }
        })
        .collect();

    (schema_fns, schema_insertions)
}

pub(super) fn generate_normalization(
    flatten_fn_name: &Ident,
    update_refs_fn_name: &Ident,
) -> TokenStream {
    quote! {
        /// Helper function to extract examples from a JSON schema
        fn #flatten_fn_name(
            mut schema: serde_json::Value,
            extracted_defs: &mut std::collections::HashMap<String, serde_json::Value>
        ) -> serde_json::Value {
            if let Some(obj) = schema.as_object_mut() {
                if let Some(defs) = obj.remove("$defs") {
                    if let Some(defs_obj) = defs.as_object() {
                        for (def_name, def_schema) in defs_obj {
                            let flattened_def = #flatten_fn_name(def_schema.clone(), extracted_defs);
                            extracted_defs.insert(def_name.clone(), flattened_def);
                        }
                    }
                }

                #update_refs_fn_name(&mut schema);
            }

            schema
        }

        /// Recursively update all $ref paths from #/$defs/ to #/components/schemas/
        fn #update_refs_fn_name(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(obj) => {
                    for (key, val) in obj.iter_mut() {
                        if key == "$ref" {
                            if let Some(ref_str) = val.as_str() {
                                if ref_str.starts_with("#/$defs/") {
                                    *val = serde_json::Value::String(
                                        ref_str.replace("#/$defs/", "#/components/schemas/")
                                    );
                                }
                            }
                        } else {
                            #update_refs_fn_name(val);
                        }
                    }
                }
                serde_json::Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        #update_refs_fn_name(item);
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

    for method in &service_def.methods {
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
        unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

        for version in &method.versions {
            let request_type = &version.request_type;
            let response_type = &version.response_type;

            unique_types.insert(quote!(#request_type).to_string(), quote!(#request_type));
            unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));
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
