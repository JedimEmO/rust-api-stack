//! REST service syntax and diagnostics.

use crate::{ast::*, static_hosting};
use quote::quote;
use syn::{Ident, LitStr, Token, Type, parse::Parse};

const DOC_COMMENT_EXPECTED: &str = "Expected doc comment in the form `/// ...`";

fn parse_label(input: syn::parse::ParseStream) -> syn::Result<String> {
    if input.peek(LitStr) {
        Ok(input.parse::<LitStr>()?.value())
    } else {
        Ok(input.parse::<Ident>()?.to_string())
    }
}

fn parse_doc_comment_attrs(
    attrs: Vec<syn::Attribute>,
    entry_kind: &str,
) -> syn::Result<Option<DocComment>> {
    let lines = attrs
        .into_iter()
        .map(|attr| parse_doc_comment_attr(attr, entry_kind))
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(DocComment::from_lines(lines))
}

fn parse_doc_comment_attr(attr: syn::Attribute, entry_kind: &str) -> syn::Result<String> {
    if !attr.path().is_ident("doc") {
        return Err(syn::Error::new_spanned(
            attr,
            format!("Only doc comments (`/// ...`) are supported before {entry_kind} definitions"),
        ));
    }

    if let syn::Meta::NameValue(name_value) = &attr.meta
        && let syn::Expr::Lit(expr_lit) = &name_value.value
        && let syn::Lit::Str(doc_line) = &expr_lit.lit
    {
        return Ok(doc_line.value());
    }

    Err(syn::Error::new_spanned(attr, DOC_COMMENT_EXPECTED))
}

impl Parse for ServiceDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let content;
        syn::braced!(content in input);

        let _ = content.parse::<Ident>()?; // "service_name"
        let _ = content.parse::<Token![:]>()?;
        let service_name = content.parse::<Ident>()?;
        let _ = content.parse::<Token![,]>()?;

        let _ = content.parse::<Ident>()?; // "base_path"
        let _ = content.parse::<Token![:]>()?;
        let base_path_lit = content.parse::<LitStr>()?;
        let base_path = base_path_lit.value();
        let _ = content.parse::<Token![,]>()?;

        let mut openapi = None;
        let mut static_hosting = static_hosting::StaticHostingConfig::default();
        let mut body_limit = None;
        let mut feature_gated = false;
        let mut require_json_content_type = true;
        let mut docs_require_auth = false;

        while content.peek(Ident) {
            let field_name = content.fork().parse::<Ident>()?;

            if field_name == "openapi" {
                let _ = content.parse::<Ident>()?; // "openapi"
                let _ = content.parse::<Token![:]>()?;

                if content.peek(syn::LitBool) {
                    let enabled = content.parse::<syn::LitBool>()?;
                    if enabled.value() {
                        openapi = Some(OpenApiConfig::Enabled);
                    }
                } else if content.peek(syn::token::Brace) {
                    let openapi_content;
                    syn::braced!(openapi_content in content);

                    let _ = openapi_content.parse::<Ident>()?; // "output"
                    let _ = openapi_content.parse::<Token![:]>()?;
                    let path = openapi_content.parse::<LitStr>()?;
                    openapi = Some(OpenApiConfig::WithPath(path.value()));
                }

                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "serve_docs" {
                let _ = content.parse::<Ident>()?; // "serve_docs"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                static_hosting.serve_docs = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "docs_path" {
                let _ = content.parse::<Ident>()?; // "docs_path"
                let _ = content.parse::<Token![:]>()?;
                let path = content.parse::<LitStr>()?;
                static_hosting.docs_path = path.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "ui_theme" {
                let _ = content.parse::<Ident>()?; // "ui_theme"
                let _ = content.parse::<Token![:]>()?;
                let theme = content.parse::<LitStr>()?;
                static_hosting.ui_theme = theme.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "body_limit" {
                let _ = content.parse::<Ident>()?; // "body_limit"
                let _ = content.parse::<Token![:]>()?;
                let limit = content.parse::<syn::LitInt>()?;
                body_limit = Some(limit.base10_parse::<usize>()?);
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "feature_gated" {
                let _ = content.parse::<Ident>()?; // "feature_gated"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                feature_gated = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "require_json_content_type" {
                let _ = content.parse::<Ident>()?; // "require_json_content_type"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                require_json_content_type = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "docs_require_auth" {
                let _ = content.parse::<Ident>()?; // "docs_require_auth"
                let _ = content.parse::<Token![:]>()?;
                let enabled = content.parse::<syn::LitBool>()?;
                docs_require_auth = enabled.value();
                let _ = content.parse::<Token![,]>()?;
            } else if field_name == "endpoints" {
                break; // Start parsing endpoints
            } else {
                return Err(syn::Error::new(
                    field_name.span(),
                    format!("Unknown field: {}", field_name),
                ));
            }
        }

        let _ = content.parse::<Ident>()?; // "endpoints"
        let _ = content.parse::<Token![:]>()?;

        let endpoints_content;
        syn::bracketed!(endpoints_content in content);

        let mut endpoints = Vec::new();
        while !endpoints_content.is_empty() {
            let endpoint = endpoints_content.parse::<EndpointDefinition>()?;
            endpoints.push(endpoint);

            if endpoints_content.peek(Token![,]) {
                let _ = endpoints_content.parse::<Token![,]>()?;
            }
        }

        Ok(ServiceDefinition {
            service_name,
            base_path,
            openapi,
            static_hosting,
            body_limit,
            feature_gated,
            require_json_content_type,
            docs_require_auth,
            endpoints,
        })
    }
}

fn parse_endpoint_path(
    input: syn::parse::ParseStream,
) -> syn::Result<(String, Vec<PathParam>, Vec<String>)> {
    let mut path_segments = Vec::new();
    let mut path_params = Vec::new();
    let mut handler_name_parts = Vec::new();

    let first_segment = input.parse::<Ident>()?;
    path_segments.push(first_segment.to_string());
    handler_name_parts.push(first_segment.to_string());

    while input.peek(Token![/]) {
        let _ = input.parse::<Token![/]>()?;

        if input.peek(syn::token::Brace) {
            let param_content;
            syn::braced!(param_content in input);

            let param_name = param_content.parse::<Ident>()?;
            let _ = param_content.parse::<Token![:]>()?;
            let param_type = param_content.parse::<Type>()?;

            path_segments.push(format!("{{{}}}", param_name));
            path_params.push(PathParam {
                name: param_name.clone(),
                param_type,
            });
            handler_name_parts.push(format!("by_{}", param_name));
        } else {
            let segment = input.parse::<Ident>()?;
            path_segments.push(segment.to_string());
            handler_name_parts.push(segment.to_string());
        }
    }

    Ok((
        format!("/{}", path_segments.join("/")),
        path_params,
        handler_name_parts,
    ))
}

fn parse_query_params(input: syn::parse::ParseStream) -> syn::Result<Vec<QueryParam>> {
    let mut query_params = Vec::new();

    if input.is_empty() {
        return Ok(query_params);
    }

    let param_name = input.parse::<Ident>()?;
    let _ = input.parse::<Token![:]>()?;
    let param_type = input.parse::<Type>()?;
    query_params.push(QueryParam {
        name: param_name,
        param_type,
    });

    while input.peek(Token![&]) || input.peek(Token![,]) {
        if input.peek(Token![&]) {
            let _ = input.parse::<Token![&]>()?;
        } else {
            let _ = input.parse::<Token![,]>()?;
        }

        if input.is_empty() {
            break;
        }

        let param_name = input.parse::<Ident>()?;
        let _ = input.parse::<Token![:]>()?;
        let param_type = input.parse::<Type>()?;
        query_params.push(QueryParam {
            name: param_name,
            param_type,
        });
    }

    Ok(query_params)
}

impl Parse for EndpointDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let docs = parse_doc_comment_attrs(input.call(syn::Attribute::parse_outer)?, "endpoint")?;

        let method_ident = input.parse::<Ident>()?;
        let method = match method_ident.to_string().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "DELETE" => HttpMethod::Delete,
            "PATCH" => HttpMethod::Patch,
            _ => {
                return Err(syn::Error::new(
                    method_ident.span(),
                    "Expected GET, POST, PUT, DELETE, or PATCH",
                ));
            }
        };

        let auth = if input.peek(syn::Ident) {
            let auth_ident = input.parse::<Ident>()?;
            match auth_ident.to_string().as_str() {
                "UNAUTHORIZED" => AuthRequirement::Unauthorized,
                "OPTIONAL_AUTH" => AuthRequirement::OptionalAuth,
                "WITH_PERMISSIONS" => {
                    let perms_content;
                    syn::parenthesized!(perms_content in input);

                    let mut permission_groups = Vec::new();

                    let first_group_content;
                    syn::bracketed!(first_group_content in perms_content);

                    let mut first_group = Vec::new();
                    while !first_group_content.is_empty() {
                        let perm = first_group_content.parse::<LitStr>()?;
                        first_group.push(perm.value());

                        if first_group_content.peek(Token![,]) {
                            let _ = first_group_content.parse::<Token![,]>()?;
                        }
                    }
                    permission_groups.push(first_group);

                    while perms_content.peek(Token![|]) {
                        let _ = perms_content.parse::<Token![|]>()?;

                        let group_content;
                        syn::bracketed!(group_content in perms_content);

                        let mut group = Vec::new();
                        while !group_content.is_empty() {
                            let perm = group_content.parse::<LitStr>()?;
                            group.push(perm.value());

                            if group_content.peek(Token![,]) {
                                let _ = group_content.parse::<Token![,]>()?;
                            }
                        }
                        permission_groups.push(group);
                    }

                    if permission_groups.len() > 1
                        && permission_groups.iter().any(|group| group.is_empty())
                    {
                        return Err(syn::Error::new(
                            auth_ident.span(),
                            "an empty permission group is only valid as the entire requirement \
                             (WITH_PERMISSIONS([]), meaning any authenticated user); mixing an \
                             empty group with non-empty groups would silently grant access to any \
                             authenticated user",
                        ));
                    }

                    AuthRequirement::WithPermissions(permission_groups)
                }
                _ => {
                    return Err(syn::Error::new(
                        auth_ident.span(),
                        "Expected UNAUTHORIZED, OPTIONAL_AUTH, or WITH_PERMISSIONS",
                    ));
                }
            }
        } else {
            return Err(syn::Error::new(
                input.span(),
                "Expected authentication requirement",
            ));
        };

        let (path, path_params, handler_name_parts) = parse_endpoint_path(input)?;

        let mut query_params = Vec::new();
        if input.peek(Token![?]) {
            let _ = input.parse::<Token![?]>()?;
            query_params = parse_query_params(input)?;
        }

        let method_str = method.as_str().to_lowercase();
        let path_str = handler_name_parts.join("_");
        let handler_name = syn::parse_str::<Ident>(&format!("{}_{}", method_str, path_str))?;

        let request_type = if input.peek(syn::token::Paren) {
            let request_content;
            syn::parenthesized!(request_content in input);
            if !request_content.is_empty() {
                Some(request_content.parse::<Type>()?)
            } else {
                None
            }
        } else {
            None
        };

        let _ = input.parse::<Token![->]>()?;
        let response_type = input.parse::<Type>()?;

        let mut version = None;
        let mut versions = Vec::new();
        let mut body_limit = None;
        let mut with_headers = false;

        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);

            while !content.is_empty() {
                let field_name = content.parse::<Ident>()?;
                let _ = content.parse::<Token![:]>()?;

                match field_name.to_string().as_str() {
                    "version" => {
                        version = Some(parse_label(&content)?);
                    }
                    "versions" => {
                        let versions_content;
                        syn::bracketed!(versions_content in content);

                        while !versions_content.is_empty() {
                            versions.push(versions_content.parse::<EndpointVersionDefinition>()?);

                            if versions_content.peek(Token![,]) {
                                let _ = versions_content.parse::<Token![,]>()?;
                            }
                        }
                    }
                    "body_limit" => {
                        let limit = content.parse::<syn::LitInt>()?;
                        body_limit = Some(limit.base10_parse::<usize>()?);
                    }
                    "headers" => {
                        with_headers = content.parse::<syn::LitBool>()?.value();
                    }
                    _ => {
                        return Err(syn::Error::new(
                            field_name.span(),
                            "Expected version, versions, body_limit, or headers",
                        ));
                    }
                }

                if content.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                }
            }
        }

        Ok(EndpointDefinition {
            docs,
            method,
            auth,
            path,
            path_params,
            query_params,
            request_type,
            response_type,
            handler_name,
            version,
            versions,
            body_limit,
            with_headers,
        })
    }
}

impl Parse for EndpointVersionDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let version = parse_label(input)?;

        let content;
        syn::braced!(content in input);

        let mut path = None;
        let mut path_params = Vec::new();
        let mut query_params = Vec::new();
        let mut request_type = None;
        let mut response_type = None;
        let mut migration_type = None;

        while !content.is_empty() {
            let field_name = content.parse::<Ident>()?;
            let _ = content.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "path" => {
                    let (parsed_path, parsed_path_params, _) = parse_endpoint_path(&content)?;
                    path = Some(parsed_path);
                    path_params = parsed_path_params;
                }
                "query" => {
                    let query_content;
                    syn::bracketed!(query_content in content);
                    query_params = parse_query_params(&query_content)?;
                }
                "body" | "request" => {
                    let parsed_type = content.parse::<Type>()?;
                    if quote!(#parsed_type).to_string() != "()" {
                        request_type = Some(parsed_type);
                    }
                }
                "response" => {
                    response_type = Some(content.parse::<Type>()?);
                }
                "migration" => {
                    migration_type = Some(content.parse::<Type>()?);
                }
                _ => {
                    return Err(syn::Error::new(
                        field_name.span(),
                        "Expected path, query, body, request, response, or migration",
                    ));
                }
            }

            if content.peek(Token![,]) {
                let _ = content.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            version,
            path: path
                .ok_or_else(|| syn::Error::new(input.span(), "Version entry is missing path"))?,
            path_params,
            query_params,
            request_type,
            response_type: response_type.ok_or_else(|| {
                syn::Error::new(input.span(), "Version entry is missing response")
            })?,
            migration_type: migration_type.ok_or_else(|| {
                syn::Error::new(input.span(), "Version entry is missing migration")
            })?,
        })
    }
}
