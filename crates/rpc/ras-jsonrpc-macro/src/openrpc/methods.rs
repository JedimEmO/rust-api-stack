use crate::{AuthRequirement, ExampleOutcome, MethodExample, ServiceDefinition};
use proc_macro2::{Ident, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

pub(super) fn generate_method_infos(
    service_def: &ServiceDefinition,
    method_info_struct_name: &Ident,
    components_mod: &Ident,
) -> Vec<TokenStream> {
    let service = service_def.service_name.to_string();
    let generators = super::local("generators");
    let method_infos: Vec<TokenStream> = service_def
        .methods
        .iter()
        .flat_map(|method| {
            let canonical_method_name = method
                .wire_name
                .clone()
                .unwrap_or_else(|| method.name.to_string());
            let canonical_version = method.version.clone();
            let canonical_version_tokens = match &canonical_version {
                Some(version) => quote! { Some(#version.to_string()) },
                None => quote! { None },
            };
            let auth_required = matches!(method.auth, AuthRequirement::WithPermissions(_));
            let auth_optional = matches!(method.auth, AuthRequirement::OptionalAuth);
            let permissions = match &method.auth {
                AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => vec![],
                AuthRequirement::WithPermissions(groups) => {
                    groups.iter().flatten().cloned().collect()
                }
            };
            let permission_groups = permission_groups_for_spec(&method.auth);
            let permission_groups_tokens = permission_groups_tokens(&permission_groups);

            let request_type = &method.request_type;
            let response_type = &method.response_type;
            let params_schema = params_schema_tokens(request_type);
            let examples = method.examples.iter().map(|example| {
                example_tokens(
                    &service,
                    &canonical_method_name,
                    request_type,
                    response_type,
                    example,
                    components_mod,
                )
            });
            let (summary, description) = match &method.docs {
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

            let mut infos = vec![quote! {
                #method_info_struct_name {
                    name: #canonical_method_name.to_string(),
                    summary: #summary,
                    description: #description,
                    auth_required: #auth_required,
                    auth_optional: #auth_optional,
                    permissions: vec![#(#permissions.to_string()),*],
                    permission_groups: #permission_groups_tokens,
                    request_type_name: stringify!(#request_type).to_string(),
                    response_type_name: stringify!(#response_type).to_string(),
                    params_schema: #params_schema,
                    result_schema: #generators.response::<#response_type>(),
                    examples: vec![#(#examples),*],
                    version: #canonical_version_tokens,
                    canonical_version: #canonical_version_tokens,
                    canonical_method: #canonical_method_name.to_string(),
                }
            }];

            infos.extend(method.versions.iter().map(|version| {
                let method_name = &version.wire_name;
                let version_label = &version.version;
                let request_type = &version.request_type;
                let response_type = &version.response_type;
                let params_schema = params_schema_tokens(request_type);
                let canonical_version = canonical_version
                    .clone()
                    .unwrap_or_else(|| "current".to_string());
                let canonical_method_name = canonical_method_name.clone();
                let permissions = permissions.clone();
                let permission_groups_tokens = permission_groups_tokens.clone();
                let summary = summary.clone();
                let description = description.clone();

                quote! {
                    #method_info_struct_name {
                        name: #method_name.to_string(),
                        summary: #summary,
                        description: #description,
                        auth_required: #auth_required,
                        auth_optional: #auth_optional,
                        permissions: vec![#(#permissions.to_string()),*],
                        permission_groups: #permission_groups_tokens,
                        request_type_name: stringify!(#request_type).to_string(),
                        response_type_name: stringify!(#response_type).to_string(),
                        params_schema: #params_schema,
                        result_schema: #generators.response::<#response_type>(),
                        examples: Vec::new(),
                        version: Some(#version_label.to_string()),
                        canonical_version: Some(#canonical_version.to_string()),
                        canonical_method: #canonical_method_name.to_string(),
                    }
                }
            }));

            infos
        })
        .collect();

    method_infos
}

pub(super) fn generate_methods(service: &str, components_mod: &Ident) -> TokenStream {
    quote! {
            let mut openrpc_methods: Vec<serde_json::Value> = Vec::new();
            for method in methods {
                let mut params = vec![];
                let mut param_structure = None;
                let mut extensions: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();

                let shape = method
                    .params_schema
                    .as_ref()
                    .map(|schema| #components_mod::classify_params(schema, &schemas));
                match &shape {
                    Some(#components_mod::ParamShape::Named(fields)) => {
                        param_structure = Some("by-name");
                        for field in fields {
                            let mut descriptor = json!({
                                "name": field.name,
                                "required": field.required,
                                "schema": field.schema
                            });
                            if let Some(description) = field.schema.get("description").cloned() {
                                descriptor["description"] = description;
                            }
                            params.push(descriptor);
                        }
                    }
                    Some(#components_mod::ParamShape::Raw) => {
                        params.push(json!({
                            "name": "params",
                            "summary": format!("Request parameters of type {}", method.request_type_name),
                            "description": "The whole `params` value. Its shape cannot be described as named \
                                parameters, so a call built from this descriptor alone would wrap it in a \
                                `params` field; send the value itself.",
                            "required": true,
                            "schema": method.params_schema.clone()
                        }));
                        extensions.insert("x-ras-params-raw".to_string(), json!(true));
                    }
                    None => {}
                }
                if let Some(schema) = &method.params_schema {
                    extensions.insert("x-ras-params-schema".to_string(), schema.clone());
                }

                let errors = #components_mod::method_errors(
                    method.params_schema.is_some(),
                    method.auth_required,
                    &method.examples,
                );
                let method_name = method.name.clone();
                let (pairings, error_examples) =
                    #components_mod::example_objects(shape.as_ref(), method.examples).map_err(
                        |(example, cause)| {
                            ras_jsonrpc_types::SpecGenerationError::example(
                                #service,
                                &method_name,
                                &example,
                                "params",
                                cause,
                            )
                        },
                    )?;

                if method.auth_required {
                    extensions.insert("x-authentication".to_string(), json!({
                        "required": true,
                        "type": "bearer"
                    }));

                    if !method.permissions.is_empty() {
                        extensions.insert("x-permissions".to_string(), json!(method.permissions));
                    }

                    if !method.permission_groups.is_empty() {
                        extensions.insert("x-permission-groups".to_string(), json!(method.permission_groups));
                    }
                } else if method.auth_optional {
                    // OPTIONAL_AUTH: authentication is honoured but not required.
                    extensions.insert("x-authentication".to_string(), json!({
                        "required": false,
                        "type": "bearer"
                    }));
                }

                if let Some(version) = &method.version {
                    extensions.insert("x-ras-version".to_string(), json!(version));
                }

                if let Some(canonical_version) = &method.canonical_version {
                    extensions.insert("x-ras-canonical-version".to_string(), json!(canonical_version));
                    extensions.insert("x-ras-canonical-method".to_string(), json!(method.canonical_method));
                }

                if !error_examples.is_empty() {
                    extensions.insert("x-ras-error-examples".to_string(), json!(error_examples));
                }

                let method_summary = method
                    .summary
                    .clone()
                    .unwrap_or_else(|| format!("Calls the {} method", method.name));

                let mut method_obj = json!({
                    "name": method.name,
                    "summary": method_summary,
                    "params": params,
                    "result": {
                        "name": "result",
                        "description": format!("Response of type {}", method.response_type_name),
                        "schema": method.result_schema
                    },
                    "errors": errors
                });

                if let Some(obj) = method_obj.as_object_mut() {
                    if let Some(param_structure) = param_structure {
                        obj.insert("paramStructure".to_string(), json!(param_structure));
                    }
                    if !pairings.is_empty() {
                        obj.insert("examples".to_string(), json!(pairings));
                    }
                    if let Some(description) = &method.description {
                        obj.insert("description".to_string(), json!(description));
                    }

                    for (key, value) in extensions {
                        obj.insert(key, value);
                    }
                }

                openrpc_methods.push(method_obj);
            }

    }
}

/// One evaluated example: a block that builds a `Example` or returns a
/// `SpecGenerationError` naming where it failed.
fn example_tokens(
    service: &str,
    method: &str,
    request_type: &syn::Type,
    response_type: &syn::Type,
    example: &MethodExample,
    components_mod: &Ident,
) -> TokenStream {
    let name = example.name.to_string();
    let context = super::local("context");
    let value = super::local("value");
    let description = match &example.docs {
        Some(docs) => {
            let description = &docs.description;
            quote! { Some(#description.to_string()) }
        }
        None => quote! { None },
    };

    // Each value is bound to its declared type, so a mismatch is reported at
    // the example; the serde bounds are reported there too.
    let params = match &example.params {
        None => quote! { None },
        Some(expr) => {
            let round_trip = quote_spanned! { expr.span()=> #components_mod::round_trip(&#value) };
            quote! {
                Some({
                    let #value: #request_type = #expr;
                    #round_trip.map_err(|cause| #context("params", cause))?
                })
            }
        }
    };

    let outcome = match &example.outcome {
        ExampleOutcome::Result(expr) => {
            let to_json = quote_spanned! { expr.span()=> #components_mod::to_json(&#value) };
            quote! {
                #components_mod::ExampleOutcome::Result({
                    let #value: #response_type = #expr;
                    #to_json.map_err(|cause| #context("result", cause))?
                })
            }
        }
        ExampleOutcome::Error {
            code,
            message,
            data,
        } => {
            let data = match data {
                Some(expr) => {
                    let to_json =
                        quote_spanned! { expr.span()=> #components_mod::to_json(&#value) };
                    quote! {
                        Some({
                            let #value = #expr;
                            #to_json.map_err(|cause| #context("error data", cause))?
                        })
                    }
                }
                None => quote! { None },
            };
            quote! {
                #components_mod::ExampleOutcome::Error {
                    code: i64::from(#code),
                    message: #message.to_string(),
                    data: #data,
                }
            }
        }
    };

    quote! {
        {
            let #context = |part: &str, cause: String| {
                ras_jsonrpc_types::SpecGenerationError::example(#service, #method, #name, part, cause)
            };
            let _ = &#context;
            #components_mod::Example {
                name: #name.to_string(),
                description: #description,
                params: #params,
                outcome: #outcome,
            }
        }
    }
}

/// The `params_schema` field: `None` for `()`, which takes no parameters.
fn params_schema_tokens(request_type: &syn::Type) -> TokenStream {
    if quote!(#request_type).to_string() == "()" {
        quote! { None }
    } else {
        let generators = super::local("generators");
        quote! { Some(#generators.request::<#request_type>()) }
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
