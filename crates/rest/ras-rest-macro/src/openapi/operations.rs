use crate::{
    AuthRequirement, EndpointDefinition, EndpointExample, EndpointExampleOutcome, PathParam,
    QueryParam, ServiceDefinition,
};
use proc_macro2::{Ident, TokenStream};
use quote::{quote, quote_spanned};
use syn::Type;
use syn::spanned::Spanned;

fn type_text(ty: &Type) -> String {
    quote!(#ty).to_string().replace(' ', "")
}

/// `Some(schema)` for a request body type, `None` without one.
fn request_schema_tokens(request_type: Option<&Type>) -> TokenStream {
    match request_type {
        Some(request_type) => {
            let generators = super::local("generators");
            quote! { Some(#generators.request::<#request_type>()) }
        }
        None => quote! { None },
    }
}

/// `Some(schema)` for a response type, `None` for `()`, which is sent as JSON `null`.
fn response_schema_tokens(response_type: &Type) -> TokenStream {
    if type_text(response_type) == "()" {
        quote! { None }
    } else {
        let generators = super::local("generators");
        quote! { Some(#generators.response::<#response_type>()) }
    }
}

fn path_param_tokens(params: &[PathParam]) -> Vec<TokenStream> {
    params
        .iter()
        .map(|param| {
            let name = param.name.to_string();
            let param_type = &param.param_type;
            let text = type_text(param_type);
            let generators = super::local("generators");
            quote! {
                (#name.to_string(), #text.to_string(), #generators.request::<#param_type>())
            }
        })
        .collect()
}

fn query_param_tokens(params: &[QueryParam]) -> Vec<TokenStream> {
    params
        .iter()
        .map(|param| {
            let name = param.name.to_string();
            let param_type = &param.param_type;
            let text = type_text(param_type);
            let required = !text.starts_with("Option<");
            let generators = super::local("generators");
            quote! {
                (#name.to_string(), #text.to_string(), #required, #generators.request::<#param_type>())
            }
        })
        .collect()
}

pub(super) fn generate_endpoint_infos(
    service_def: &ServiceDefinition,
    endpoint_info_struct_name: &Ident,
    components_mod: &Ident,
) -> Vec<TokenStream> {
    let service = service_def.service_name.to_string();
    service_def
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
            let permissions: Vec<String> = match &endpoint.auth {
                AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => vec![],
                AuthRequirement::WithPermissions(groups) => {
                    groups.iter().flatten().cloned().collect()
                }
            };
            let permission_groups = permission_groups_for_spec(&endpoint.auth);
            let permission_groups_tokens = permission_groups_tokens(&permission_groups);

            let request_schema = request_schema_tokens(endpoint.request_type.as_ref());
            let response_schema = response_schema_tokens(&endpoint.response_type);
            let path_params = path_param_tokens(&endpoint.path_params);
            let query_params = query_param_tokens(&endpoint.query_params);
            let examples = endpoint
                .examples
                .iter()
                .map(|example| example_tokens(&service, endpoint, example, components_mod));

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
                    request_schema: #request_schema,
                    response_schema: #response_schema,
                    path_params: vec![#(#path_params),*],
                    query_params: vec![#(#query_params),*],
                    examples: vec![#(#examples),*],
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
                let request_schema = request_schema_tokens(version.request_type.as_ref());
                let response_schema = response_schema_tokens(&version.response_type);
                let path_params = path_param_tokens(&version.path_params);
                let query_params = query_param_tokens(&version.query_params);
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
                        request_schema: #request_schema,
                        response_schema: #response_schema,
                        path_params: vec![#(#path_params),*],
                        query_params: vec![#(#query_params),*],
                        examples: Vec::new(),
                        version: Some(#version_label.to_string()),
                        canonical_version: Some(#canonical_version.to_string()),
                        canonical_path: #canonical_path.to_string(),
                    }
                }
            }));

            infos
        })
        .collect()
}

pub(super) fn generate_paths(components_mod: &Ident) -> TokenStream {
    quote! {
            let mut paths = serde_json::Map::new();

            for endpoint in endpoints {
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
                // `()` is sent as JSON `null`, which OpenAPI 3.0 can only express as a nullable enum.
                let response_schema = endpoint.response_schema.unwrap_or_else(|| json!({
                    "nullable": true,
                    "enum": [null],
                    "description": "Always JSON `null`."
                }));
                let example_response_schema = response_schema.clone();
                let mut operation = json!({
                    "summary": operation_summary,
                    "description": operation_description,
                    "operationId": format!("{}_{}", method_lower, endpoint.path.replace("/", "_").replace("{", "").replace("}", "").trim_start_matches('_')),
                    "responses": {
                        "200": {
                            "description": "Successful response",
                            "content": {
                                "application/json": {
                                    "schema": response_schema
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

                for (param_name, param_type, schema) in endpoint.path_params {
                    parameters.push(json!({
                        "name": param_name,
                        "in": "path",
                        "required": true,
                        "description": format!("Path parameter of type {}", param_type),
                        "schema": schema
                    }));
                }

                for (param_name, param_type, required, schema) in endpoint.query_params {
                    parameters.push(json!({
                        "name": param_name,
                        "in": "query",
                        "required": required,
                        "description": format!("Query parameter of type {}", param_type),
                        "schema": schema
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

                if let Some(request_schema) = endpoint.request_schema {
                    if endpoint.method != "GET" {
                        operation["requestBody"] = json!({
                            "description": "Request body",
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": request_schema
                                }
                            }
                        });
                    }
                }

                #components_mod::describe_rest_error_responses(&mut operation);
                #components_mod::apply_rest_examples(
                    &mut operation,
                    &example_response_schema,
                    endpoint.examples,
                );

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

/// One evaluated example: a block that builds a `RestExample` or returns a
/// `SpecGenerationError` naming where it failed.
fn example_tokens(
    service: &str,
    endpoint: &EndpointDefinition,
    example: &EndpointExample,
    components_mod: &Ident,
) -> TokenStream {
    let operation = format!("{} {}", endpoint.method.as_str(), endpoint.path);
    let name = example.name.to_string();
    let context = super::local("context");
    let value = super::local("value");
    let (summary, description) = match &example.docs {
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

    // Each value is bound to its declared type, so a mismatch is reported at
    // the example.
    let param_value = |param_name: &Ident, param_type: &Type, expr: &syn::Expr, part: String| {
        let to_json = quote_spanned! { expr.span()=> #components_mod::to_json(&#value) };
        let key = param_name.to_string();
        quote! {
            (#key.to_string(), {
                let #value: #param_type = #expr;
                #to_json.map_err(|cause| #context(#part, cause))?
            })
        }
    };
    let path = example.path.iter().map(|(param_name, expr)| {
        let param_type = &endpoint
            .path_params
            .iter()
            .find(|param| &param.name == param_name)
            .expect("the parser checked path parameter names")
            .param_type;
        param_value(
            param_name,
            param_type,
            expr,
            format!("path parameter {param_name}"),
        )
    });
    let query = example.query.iter().map(|(param_name, expr)| {
        let param_type = &endpoint
            .query_params
            .iter()
            .find(|param| &param.name == param_name)
            .expect("the parser checked query parameter names")
            .param_type;
        param_value(
            param_name,
            param_type,
            expr,
            format!("query parameter {param_name}"),
        )
    });

    let body = match (&example.body, &endpoint.request_type) {
        (Some(expr), Some(request_type)) => {
            let round_trip = quote_spanned! { expr.span()=> #components_mod::round_trip(&#value) };
            quote! {
                Some({
                    let #value: #request_type = #expr;
                    #round_trip.map_err(|cause| #context("body", cause))?
                })
            }
        }
        _ => quote! { None },
    };

    let response_type = &endpoint.response_type;
    let (status, response, error) = match &example.outcome {
        EndpointExampleOutcome::Success { status, response } => {
            let response = match response {
                Some(expr) => {
                    let to_json =
                        quote_spanned! { expr.span()=> #components_mod::to_json(&#value) };
                    quote! {
                        Some({
                            let #value: #response_type = #expr;
                            #to_json.map_err(|cause| #context("response", cause))?
                        })
                    }
                }
                None => quote! { None },
            };
            (*status, response, quote! { None })
        }
        EndpointExampleOutcome::Error { status, message } => (
            *status,
            quote! { None },
            quote! { Some(#message.to_string()) },
        ),
    };

    quote! {
        {
            let #context = |part: &str, cause: String| {
                ras_rest_core::SpecGenerationError::example(#service, #operation, #name, part, cause)
            };
            let _ = &#context;
            #components_mod::RestExample {
                name: #name.to_string(),
                summary: #summary,
                description: #description,
                path: vec![#(#path),*],
                query: vec![#(#query),*],
                body: #body,
                status: #status,
                response: #response,
                error: #error,
            }
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
