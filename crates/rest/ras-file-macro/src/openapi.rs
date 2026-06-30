use crate::parser::{
    AuthRequirement, FileServiceDefinition, MaxBytes, OpenApiConfig, Operation, UploadPart,
    UploadPartKind,
};
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashMap;

pub fn generate_openapi_code(
    service_def: &FileServiceDefinition,
    config: &OpenApiConfig,
) -> TokenStream {
    let service_name = &service_def.service_name;
    let base_path = service_def.base_path.value();
    let openapi_fn_name = quote::format_ident!(
        "generate_{}_openapi",
        service_name.to_string().to_lowercase()
    );
    let openapi_to_file_fn_name = quote::format_ident!(
        "generate_{}_openapi_to_file",
        service_name.to_string().to_lowercase()
    );
    let endpoint_info_name = quote::format_ident!("{}OpenApiEndpointInfo", service_name);
    let part_info_name = quote::format_ident!("{}OpenApiPartInfo", service_name);

    let output_path_code = match config {
        OpenApiConfig::Enabled => {
            let service_name_lower = service_name.to_string().to_lowercase();
            quote! { format!("target/openapi/{}.json", #service_name_lower) }
        }
        OpenApiConfig::WithPath(path) => quote! { #path.to_string() },
    };

    let unique_types = collect_schema_types(service_def);
    let schema_fns = generate_schema_fns(service_name, &unique_types);
    let schema_insertions = generate_schema_insertions(service_name, &unique_types);
    let endpoint_infos = service_def.endpoints.iter().map(|endpoint| {
        let method = match endpoint.operation {
            Operation::Upload { .. } => "POST",
            Operation::Download { .. } => "GET",
        };
        let operation = match endpoint.operation {
            Operation::Upload { .. } => "upload",
            Operation::Download { .. } => "download",
        };
        let path = endpoint.path.value();
        let auth_required = matches!(endpoint.auth, AuthRequirement::WithPermissions(_));
        // OPTIONAL_AUTH advertises an *optional* security requirement.
        let auth_optional = matches!(endpoint.auth, AuthRequirement::OptionalAuth);
        let permissions = permissions_for_openapi(&endpoint.auth);
        let permission_groups = permission_groups_for_openapi(&endpoint.auth);
        let permission_groups_tokens = permission_groups_tokens(&permission_groups);

        let path_params = endpoint.path_params.iter().map(|param| {
            let name = param.name.to_string();
            let param_ty = &param.ty;
            let ty = sanitize_type_name(&quote!(#param_ty).to_string());
            quote! { (#name.to_string(), #ty.to_string()) }
        });

        match &endpoint.operation {
            Operation::Upload {
                config,
                response_type,
            } => {
                let response_type_name = sanitize_type_name(&quote!(#response_type).to_string());
                let max_total = match config.max_total_bytes {
                    MaxBytes::Limited(limit) => quote! { Some(#limit as u64) },
                    MaxBytes::Unlimited => quote! { None },
                };
                let reject_unknown = config.reject_unknown_fields;
                let parts = config
                    .parts
                    .iter()
                    .map(|part| part_info_tokens(part, &part_info_name));
                quote! {
                    #endpoint_info_name {
                        method: #method.to_string(),
                        operation: #operation.to_string(),
                        path: #path.to_string(),
                        auth_required: #auth_required,
                        auth_optional: #auth_optional,
                        permissions: vec![#(#permissions.to_string()),*],
                        permission_groups: #permission_groups_tokens,
                        path_params: vec![#(#path_params),*],
                        response_type_name: Some(#response_type_name.to_string()),
                        max_total_bytes: #max_total,
                        reject_unknown_fields: #reject_unknown,
                        parts: vec![#(#parts),*],
                        download_content_types: vec![],
                        download_ranges: false,
                    }
                }
            }
            Operation::Download { config } => {
                let content_types = config.content_types.iter();
                let ranges = config.ranges;
                quote! {
                    #endpoint_info_name {
                        method: #method.to_string(),
                        operation: #operation.to_string(),
                        path: #path.to_string(),
                        auth_required: #auth_required,
                        auth_optional: #auth_optional,
                        permissions: vec![#(#permissions.to_string()),*],
                        permission_groups: #permission_groups_tokens,
                        path_params: vec![#(#path_params),*],
                        response_type_name: None,
                        max_total_bytes: None,
                        reject_unknown_fields: true,
                        parts: vec![],
                        download_content_types: vec![#(#content_types.to_string()),*],
                        download_ranges: #ranges,
                    }
                }
            }
        }
    });

    quote! {
        #[derive(serde::Serialize)]
        struct #part_info_name {
            kind: String,
            name: String,
            type_name: Option<String>,
            required: bool,
            max_count: usize,
            max_bytes: u64,
            content_types: Vec<String>,
        }

        #[derive(serde::Serialize)]
        struct #endpoint_info_name {
            method: String,
            operation: String,
            path: String,
            auth_required: bool,
            auth_optional: bool,
            permissions: Vec<String>,
            permission_groups: Vec<Vec<String>>,
            path_params: Vec<(String, String)>,
            response_type_name: Option<String>,
            max_total_bytes: Option<u64>,
            reject_unknown_fields: bool,
            parts: Vec<#part_info_name>,
            download_content_types: Vec<String>,
            download_ranges: bool,
        }

        fn fix_schema_refs(value: &mut serde_json::Value, schemas: &mut serde_json::Map<String, serde_json::Value>) {
            match value {
                serde_json::Value::Object(obj) => {
                    if let Some(defs) = obj.remove("$defs").or_else(|| obj.remove("definitions")) {
                        if let serde_json::Value::Object(defs_obj) = defs {
                            for (name, mut schema) in defs_obj {
                                fix_schema_refs(&mut schema, schemas);
                                schemas.insert(name, schema);
                            }
                        }
                    }

                    if let Some(serde_json::Value::String(reference)) = obj.get_mut("$ref") {
                        if reference.starts_with("#/$defs/") {
                            *reference = format!("#/components/schemas/{}", reference.trim_start_matches("#/$defs/"));
                        } else if reference.starts_with("#/definitions/") {
                            *reference = format!("#/components/schemas/{}", reference.trim_start_matches("#/definitions/"));
                        }
                    }

                    obj.remove("$schema");

                    for value in obj.values_mut() {
                        fix_schema_refs(value, schemas);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        fix_schema_refs(value, schemas);
                    }
                }
                _ => {}
            }
        }

        #(#schema_fns)*

        pub fn #openapi_fn_name() -> serde_json::Value {
            use serde_json::json;
            use std::collections::HashMap;

            let endpoints: Vec<#endpoint_info_name> = vec![#(#endpoint_infos),*];
            let mut schemas = HashMap::new();
            schemas.insert("BinaryFileResponse".to_string(), json!({
                "type": "string",
                "format": "binary",
                "description": "Binary file content"
            }));
            #(#schema_insertions)*

            let mut final_schemas = serde_json::Map::new();
            for (name, mut schema) in schemas {
                fix_schema_refs(&mut schema, &mut final_schemas);
                final_schemas.insert(name, schema);
            }

            let mut paths = serde_json::Map::new();

            for endpoint in &endpoints {
                let path_item = paths.entry(endpoint.path.clone()).or_insert_with(|| json!({}));
                let method_lower = endpoint.method.to_lowercase();
                let mut operation = json!({
                    "summary": format!("{} {}", endpoint.operation, endpoint.path),
                    "operationId": format!("{}_{}", endpoint.operation, endpoint.path.replace("/", "_").replace("{", "").replace("}", "").trim_start_matches('_')),
                    "tags": ["File Operations"],
                });

                let mut parameters = vec![];
                for (name, type_name) in &endpoint.path_params {
                    parameters.push(json!({
                        "name": name,
                        "in": "path",
                        "required": true,
                        "schema": { "$ref": format!("#/components/schemas/{}", type_name) },
                    }));
                }
                if !parameters.is_empty() {
                    operation["parameters"] = json!(parameters);
                }

                if endpoint.operation == "upload" {
                    let mut properties = serde_json::Map::new();
                    let mut required = vec![];
                    let mut encoding = serde_json::Map::new();

                    for part in &endpoint.parts {
                        let schema = match part.kind.as_str() {
                            "file" if part.max_count > 1 => json!({
                                "type": "array",
                                "items": { "type": "string", "format": "binary" },
                                "maxItems": part.max_count,
                            }),
                            "file" => json!({ "type": "string", "format": "binary" }),
                            "text" => json!({ "type": "string", "maxLength": part.max_bytes }),
                            "json" => json!({ "$ref": format!("#/components/schemas/{}", part.type_name.as_ref().expect("json type")) }),
                            _ => json!({ "type": "string" }),
                        };
                        properties.insert(part.name.clone(), schema);

                        if !part.content_types.is_empty() {
                            encoding.insert(part.name.clone(), json!({
                                "contentType": part.content_types.join(", "),
                            }));
                        }

                        if part.required {
                            required.push(part.name.clone());
                        }
                    }

                    operation["requestBody"] = json!({
                        "required": true,
                        "content": {
                            "multipart/form-data": {
                                "schema": {
                                    "type": "object",
                                    "properties": properties,
                                    "required": required,
                                },
                                "encoding": encoding,
                            }
                        }
                    });

                    operation["x-ras-file"] = json!({
                        "maxTotalBytes": endpoint.max_total_bytes,
                        "rejectUnknownFields": endpoint.reject_unknown_fields,
                        "parts": endpoint.parts,
                    });

                    operation["responses"] = json!({
                        "200": {
                            "description": "Successful upload",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": format!("#/components/schemas/{}", endpoint.response_type_name.as_ref().expect("upload response type"))
                                    }
                                }
                            }
                        },
                        "400": { "description": "Bad request" },
                        "401": { "description": "Unauthorized" },
                        "403": { "description": "Forbidden" },
                        "413": { "description": "Payload too large" },
                        "415": { "description": "Unsupported media type" },
                        "500": { "description": "Internal server error" }
                    });
                } else {
                    operation["x-ras-file"] = json!({
                        "contentTypes": endpoint.download_content_types,
                        "ranges": endpoint.download_ranges,
                    });
                    operation["responses"] = json!({
                        "200": {
                            "description": "File download",
                            "content": {
                                "application/octet-stream": {
                                    "schema": { "$ref": "#/components/schemas/BinaryFileResponse" }
                                }
                            }
                        },
                        "206": { "description": "Partial content" },
                        "304": { "description": "Not modified" },
                        "400": { "description": "Bad request" },
                        "401": { "description": "Unauthorized" },
                        "403": { "description": "Forbidden" },
                        "404": { "description": "File not found" },
                        "412": { "description": "Precondition failed" },
                        "500": { "description": "Internal server error" }
                    });
                }

                if endpoint.auth_required {
                    operation["security"] = json!([{ "bearerAuth": [] }]);
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
                    "title": format!("{} File Service API", stringify!(#service_name)),
                    "version": "2.0.0",
                    "description": format!("OpenAPI 3.0 specification for the {} file service", stringify!(#service_name))
                },
                "servers": [{
                    "url": #base_path,
                    "description": "File service base path"
                }],
                "paths": paths,
                "components": {
                    "schemas": final_schemas,
                    "securitySchemes": {
                        "bearerAuth": {
                            "type": "http",
                            "scheme": "bearer"
                        }
                    }
                },
                "tags": [{
                    "name": "File Operations",
                    "description": "File upload and download operations"
                }]
            })
        }

        pub fn #openapi_to_file_fn_name() -> std::io::Result<()> {
            let doc = #openapi_fn_name();
            let output_path = #output_path_code;
            if let Some(parent) = std::path::Path::new(&output_path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output_path, serde_json::to_string_pretty(&doc)?)?;
            Ok(())
        }
    }
}

pub fn generate_schema_impl_checks(service_def: &FileServiceDefinition) -> TokenStream {
    let unique_types = collect_schema_types(service_def);
    let checks = unique_types.values().map(|ty| {
        quote! {
            const _: () = {
                fn _assert_json_schema<T: schemars::JsonSchema>() {}
                fn _check() {
                    _assert_json_schema::<#ty>();
                }
            };
        }
    });

    quote! { #(#checks)* }
}

fn collect_schema_types(service_def: &FileServiceDefinition) -> HashMap<String, TokenStream> {
    let mut unique_types = HashMap::new();

    for endpoint in &service_def.endpoints {
        match &endpoint.operation {
            Operation::Upload {
                response_type,
                config,
            } => {
                unique_types.insert(quote!(#response_type).to_string(), quote!(#response_type));

                for part in &config.parts {
                    if let Some(ty) = &part.ty {
                        unique_types.insert(quote!(#ty).to_string(), quote!(#ty));
                    }
                }
            }
            Operation::Download { .. } => {}
        }

        for path_param in &endpoint.path_params {
            let ty = &path_param.ty;
            unique_types.insert(quote!(#ty).to_string(), quote!(#ty));
        }
    }

    unique_types
}

fn generate_schema_fns(
    service_name: &syn::Ident,
    unique_types: &HashMap<String, TokenStream>,
) -> Vec<TokenStream> {
    unique_types
        .iter()
        .map(|(type_name, type_tokens)| {
            let sanitized_name = sanitize_type_name(type_name);
            let fn_name = quote::format_ident!(
                "_generate_schema_for_{}_{}",
                service_name.to_string().to_lowercase(),
                sanitized_name
            );
            quote! {
                fn #fn_name() -> serde_json::Value {
                    serde_json::to_value(schemars::schema_for!(#type_tokens)).unwrap_or_else(|_| {
                        serde_json::json!({
                            "type": "object",
                            "description": format!("Schema for {}", #type_name)
                        })
                    })
                }
            }
        })
        .collect()
}

fn generate_schema_insertions(
    service_name: &syn::Ident,
    unique_types: &HashMap<String, TokenStream>,
) -> Vec<TokenStream> {
    unique_types
        .keys()
        .map(|type_name| {
            let sanitized_name = sanitize_type_name(type_name);
            let fn_name = quote::format_ident!(
                "_generate_schema_for_{}_{}",
                service_name.to_string().to_lowercase(),
                sanitized_name
            );
            quote! {
                schemas.insert(#sanitized_name.to_string(), #fn_name());
            }
        })
        .collect()
}

fn part_info_tokens(part: &UploadPart, part_info_name: &syn::Ident) -> TokenStream {
    let kind = match part.kind {
        UploadPartKind::File => "file",
        UploadPartKind::Json => "json",
        UploadPartKind::Text => "text",
    };
    let name = part.name.to_string();
    let type_name = part
        .ty
        .as_ref()
        .map(|ty| sanitize_type_name(&quote!(#ty).to_string()));
    let type_name_tokens = match type_name {
        Some(type_name) => quote! { Some(#type_name.to_string()) },
        None => quote! { None },
    };
    let required = part.required;
    let max_count = part.max_count;
    let max_bytes = part.max_bytes;
    let content_types = part.content_types.iter();

    quote! {
        #part_info_name {
            kind: #kind.to_string(),
            name: #name.to_string(),
            type_name: #type_name_tokens,
            required: #required,
            max_count: #max_count,
            max_bytes: #max_bytes,
            content_types: vec![#(#content_types.to_string()),*],
        }
    }
}

fn permissions_for_openapi(auth: &AuthRequirement) -> Vec<String> {
    match auth {
        AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => vec![],
        AuthRequirement::WithPermissions(groups) => groups.iter().flatten().cloned().collect(),
    }
}

fn permission_groups_for_openapi(auth: &AuthRequirement) -> Vec<Vec<String>> {
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

fn sanitize_type_name(type_name: &str) -> String {
    if type_name == "()" {
        "Unit".to_string()
    } else {
        type_name
            .replace("::", "_")
            .replace('<', "_")
            .replace(['>', ' '], "")
            .replace([',', '(', ')'], "_")
    }
}
