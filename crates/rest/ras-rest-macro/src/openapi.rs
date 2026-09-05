//! OpenAPI 3.0 document generation module
//!
//! This module provides functionality to generate OpenAPI 3.0 specification documents
//! from the rest_service macro definitions.

use crate::{AuthRequirement, OpenApiConfig, ServiceDefinition};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;

/// Generates OpenAPI document creation code
pub fn generate_openapi_code(
    service_def: &ServiceDefinition,
    config: &OpenApiConfig,
) -> TokenStream {
    let service_name = &service_def.service_name;
    let openapi_fn_name = quote::format_ident!(
        "generate_{}_openapi",
        service_name.to_string().to_lowercase()
    );
    let openapi_to_file_fn_name = quote::format_ident!(
        "generate_{}_openapi_to_file",
        service_name.to_string().to_lowercase()
    );
    let endpoint_info_struct_name = quote::format_ident!("{}OpenApiEndpointInfo", service_name);

    let output_path_code = match config {
        OpenApiConfig::Enabled => {
            let service_name_lower = service_name.to_string().to_lowercase();
            quote! {
                format!("target/openapi/{}.json", #service_name_lower)
            }
        }
        OpenApiConfig::WithPath(path) => {
            quote! {
                #path.to_string()
            }
        }
    };

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

    let sanitize_type_name = |type_name: &str| -> String {
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
    };

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

    let endpoint_infos: Vec<TokenStream> = service_def
        .endpoints
        .iter()
        .flat_map(|endpoint| {
            let method = endpoint.method.as_str();
            let path = &endpoint.path;
            let canonical_version = endpoint.version.clone();
            let canonical_version_tokens = match &canonical_version {
                Some(version) => quote! { Some(#version.to_string()) },
                None => quote! { None },
            };
            let (summary, description) = match &endpoint.docs {
                Some(docs) => {
                    let summary = &docs.summary;
                    let description = &docs.description;
                    (
                        quote! { Some(#summary.to_string()) },
                        quote! { Some(#description.to_string()) },
                    )
                }
                None => (quote! { None }, quote! { None }),
            };
            let auth_required = matches!(endpoint.auth, AuthRequirement::WithPermissions(_));
            // OPTIONAL_AUTH advertises an *optional* security requirement.
            let auth_optional = matches!(endpoint.auth, AuthRequirement::OptionalAuth);
            let permissions = match &endpoint.auth {
                AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => vec![],
                AuthRequirement::WithPermissions(groups) => {
                    groups.iter().flatten().cloned().collect()
                }
            };
            let permission_groups = permission_groups_for_spec(&endpoint.auth);
            let permission_groups_tokens = permission_groups_tokens(&permission_groups);

            let request_type_name = if let Some(request_type) = &endpoint.request_type {
                sanitize_type_name(&quote!(#request_type).to_string())
            } else {
                "Unit".to_string()
            };

            let response_type = &endpoint.response_type;
            let response_type_name = if quote!(#response_type).to_string() == "()" {
                "Unit".to_string()
            } else {
                sanitize_type_name(&quote!(#response_type).to_string())
            };
            let path_param_infos: Vec<TokenStream> = endpoint
                .path_params
                .iter()
                .map(|param| {
                    let param_name = param.name.to_string();
                    let param_type = &param.param_type;
                    let param_type_str = sanitize_type_name(&quote!(#param_type).to_string());
                    quote! {
                        (#param_name.to_string(), #param_type_str.to_string())
                    }
                })
                .collect();

            let query_param_infos: Vec<TokenStream> = endpoint
                .query_params
                .iter()
                .map(|param| {
                    let param_name = param.name.to_string();
                    let param_type = &param.param_type;
                    let param_type_str = sanitize_type_name(&quote!(#param_type).to_string());
                    quote! {
                        (#param_name.to_string(), #param_type_str.to_string())
                    }
                })
                .collect();

            let mut infos = vec![quote! {
                #endpoint_info_struct_name {
                    method: #method.to_string(),
                    path: #path.to_string(),
                    summary: #summary,
                    description: #description,
                    auth_required: #auth_required,
                    auth_optional: #auth_optional,
                    permissions: vec![#(#permissions.to_string()),*],
                    permission_groups: #permission_groups_tokens,
                    request_type_name: #request_type_name.to_string(),
                    response_type_name: #response_type_name.to_string(),
                    path_params: vec![#(#path_param_infos),*] as Vec<(String, String)>,
                    query_params: vec![#(#query_param_infos),*] as Vec<(String, String)>,
                    version: #canonical_version_tokens,
                    canonical_version: #canonical_version_tokens,
                    canonical_path: #path.to_string(),
                }
            }];

            infos.extend(endpoint.versions.iter().map(|version| {
                let path = &version.path;
                let version_label = &version.version;
                let canonical_version = canonical_version
                    .clone()
                    .unwrap_or_else(|| "current".to_string());
                let canonical_path = endpoint.path.clone();
                let request_type_name = if let Some(request_type) = &version.request_type {
                    sanitize_type_name(&quote!(#request_type).to_string())
                } else {
                    "Unit".to_string()
                };
                let response_type = &version.response_type;
                let response_type_name = if quote!(#response_type).to_string() == "()" {
                    "Unit".to_string()
                } else {
                    sanitize_type_name(&quote!(#response_type).to_string())
                };
                let path_param_infos: Vec<TokenStream> = version
                    .path_params
                    .iter()
                    .map(|param| {
                        let param_name = param.name.to_string();
                        let param_type = &param.param_type;
                        let param_type_str = sanitize_type_name(&quote!(#param_type).to_string());
                        quote! {
                            (#param_name.to_string(), #param_type_str.to_string())
                        }
                    })
                    .collect();
                let query_param_infos: Vec<TokenStream> = version
                    .query_params
                    .iter()
                    .map(|param| {
                        let param_name = param.name.to_string();
                        let param_type = &param.param_type;
                        let param_type_str = sanitize_type_name(&quote!(#param_type).to_string());
                        quote! {
                            (#param_name.to_string(), #param_type_str.to_string())
                        }
                    })
                    .collect();
                let permissions = permissions.clone();
                let permission_groups_tokens = permission_groups_tokens.clone();
                let summary = summary.clone();
                let description = description.clone();

                quote! {
                    #endpoint_info_struct_name {
                        method: #method.to_string(),
                        path: #path.to_string(),
                        summary: #summary,
                        description: #description,
                        auth_required: #auth_required,
                        auth_optional: #auth_optional,
                        permissions: vec![#(#permissions.to_string()),*],
                        permission_groups: #permission_groups_tokens,
                        request_type_name: #request_type_name.to_string(),
                        response_type_name: #response_type_name.to_string(),
                        path_params: vec![#(#path_param_infos),*] as Vec<(String, String)>,
                        query_params: vec![#(#query_param_infos),*] as Vec<(String, String)>,
                        version: Some(#version_label.to_string()),
                        canonical_version: Some(#canonical_version.to_string()),
                        canonical_path: #canonical_path.to_string(),
                    }
                }
            }));

            infos
        })
        .collect();

    quote! {
        #[derive(serde::Serialize)]
        struct #endpoint_info_struct_name {
            method: String,
            path: String,
            summary: Option<String>,
            description: Option<String>,
            auth_required: bool,
            auth_optional: bool,
            permissions: Vec<String>,
            permission_groups: Vec<Vec<String>>,
            request_type_name: String,
            response_type_name: String,
            path_params: Vec<(String, String)>, // (name, type)
            query_params: Vec<(String, String)>, // (name, type)
            version: Option<String>,
            canonical_version: Option<String>,
            canonical_path: String,
        }

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

        #(#schema_fns)*

        /// Generate OpenAPI 3.0 document for this service
        pub fn #openapi_fn_name() -> serde_json::Value {
            use serde_json::json;
            use schemars::{schema_for, JsonSchema};
            use std::collections::HashMap;

            let endpoints: Vec<#endpoint_info_struct_name> = vec![
                #(#endpoint_infos),*
            ];

            let mut schemas = HashMap::new();

            #(#schema_insertions)*

            let mut final_schemas = serde_json::Map::new();
            for (name, mut schema) in schemas {
                fix_schema_refs(&mut schema, &mut final_schemas);
                fix_option_types(&mut schema);
                final_schemas.insert(name, schema);
            }

            let mut paths = serde_json::Map::new();

            for endpoint in &endpoints {
                let path_item = paths.entry(endpoint.path.clone()).or_insert_with(|| json!({}));

                let method_lower = endpoint.method.to_lowercase();
                let operation_summary = endpoint
                    .summary
                    .clone()
                    .unwrap_or_else(|| format!("{} {}", endpoint.method, endpoint.path));
                let operation_description = endpoint
                    .description
                    .clone()
                    .unwrap_or_else(|| format!("Handles {} requests to {}", endpoint.method, endpoint.path));
                let mut operation = json!({
                    "summary": operation_summary,
                    "description": operation_description,
                    "operationId": format!("{}_{}", method_lower, endpoint.path.replace("/", "_").replace("{", "").replace("}", "").trim_start_matches('_')),
                    "responses": {
                        "200": {
                            "description": "Successful response",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": format!("#/components/schemas/{}", endpoint.response_type_name)
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "Bad request"
                        },
                        "401": {
                            "description": "Unauthorized"
                        },
                        "403": {
                            "description": "Forbidden"
                        },
                        "500": {
                            "description": "Internal server error"
                        }
                    }
                });

                let mut parameters = vec![];

                for (param_name, param_type) in &endpoint.path_params {
                    parameters.push(json!({
                        "name": param_name,
                        "in": "path",
                        "required": true,
                        "description": format!("Path parameter of type {}", param_type),
                        "schema": {
                            "$ref": format!("#/components/schemas/{}", param_type)
                        }
                    }));
                }

                for (param_name, param_type) in &endpoint.query_params {
                    let is_optional = param_type.starts_with("Option_") || param_type.starts_with("Option<") || param_type.starts_with("Option <");
                    parameters.push(json!({
                        "name": param_name,
                        "in": "query",
                        "required": !is_optional,
                        "description": format!("Query parameter of type {}", param_type),
                        "schema": {
                            "$ref": format!("#/components/schemas/{}", param_type)
                        }
                    }));
                }

                if !parameters.is_empty() {
                    operation["parameters"] = json!(parameters);
                }

                if let Some(version) = &endpoint.version {
                    operation["x-ras-version"] = json!(version);
                }

                if let Some(canonical_version) = &endpoint.canonical_version {
                    operation["x-ras-canonical-version"] = json!(canonical_version);
                    operation["x-ras-canonical-path"] = json!(endpoint.canonical_path);
                }

                if endpoint.method != "GET" && endpoint.request_type_name != "Unit" {
                    operation["requestBody"] = json!({
                        "description": "Request body",
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": format!("#/components/schemas/{}", endpoint.request_type_name)
                                }
                            }
                        }
                    });
                }

                if endpoint.auth_required {
                    operation["security"] = json!([{
                        "bearerAuth": []
                    }]);

                    if !endpoint.permissions.is_empty() {
                        operation["x-permissions"] = json!(endpoint.permissions);
                    }

                    if !endpoint.permission_groups.is_empty() {
                        operation["x-permission-groups"] = json!(endpoint.permission_groups);
                    }
                } else if endpoint.auth_optional {
                    // OPTIONAL_AUTH: anonymous is acceptable ({}), and a bearer is honoured.
                    operation["security"] = json!([{}, { "bearerAuth": [] }]);
                }

                path_item[method_lower] = operation;
            }

            json!({
                "openapi": "3.0.3",
                "info": {
                    "title": format!("{} REST API", stringify!(#service_name)),
                    "version": "1.0.0",
                    "description": format!("OpenAPI 3.0 specification for the {} service", stringify!(#service_name))
                },
                "paths": paths,
                "components": {
                    "schemas": final_schemas,
                    "securitySchemes": {
                        "bearerAuth": {
                            "type": "http",
                            "scheme": "bearer",
                            "description": "Bearer token for authentication"
                        }
                    }
                }
            })
        }

        /// Write OpenAPI document to the target directory
        pub fn #openapi_to_file_fn_name() -> std::io::Result<()> {
            let doc = #openapi_fn_name();
            let output_path = #output_path_code;

            if let Some(parent) = std::path::Path::new(&output_path).parent() {
                std::fs::create_dir_all(parent)?;
            }

            let json_string = serde_json::to_string_pretty(&doc)?;
            std::fs::write(&output_path, &json_string)?;

            println!("Generated OpenAPI document at: {}", output_path);

            Ok(())
        }

    }
}

fn permission_groups_for_spec(auth: &AuthRequirement) -> Vec<Vec<String>> {
    match auth {
        AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => vec![],
        AuthRequirement::WithPermissions(groups) => groups.clone(),
    }
}

fn permission_groups_tokens(groups: &[Vec<String>]) -> TokenStream {
    let groups = groups
        .iter()
        .map(|group| quote! { vec![#(#group.to_string()),*] });
    quote! { vec![#(#groups),*] }
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
