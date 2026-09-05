use super::schema::sanitize_type_name;
use crate::{AuthRequirement, ServiceDefinition};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub(super) fn generate_endpoint_infos(
    service_def: &ServiceDefinition,
    endpoint_info_struct_name: &Ident,
) -> Vec<TokenStream> {
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

    endpoint_infos
}

pub(super) fn generate_paths() -> TokenStream {
    quote! {
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
