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
        let mut examples = Vec::new();

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
                    "examples" => {
                        examples = parse_endpoint_examples(
                            &content,
                            &path_params,
                            &query_params,
                            request_type.as_ref(),
                            &response_type,
                        )?;
                    }
                    _ => {
                        return Err(syn::Error::new(
                            field_name.span(),
                            "Expected version, versions, body_limit, headers, or examples",
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
            examples,
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
                "examples" => {
                    return Err(syn::Error::new(
                        field_name.span(),
                        "examples belong on the current endpoint only; \
                         legacy versions are documented without examples",
                    ));
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

fn set_once<T>(slot: &mut Option<T>, field: &Ident, value: T) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new(
            field.span(),
            format!("`{field}` is given more than once"),
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn is_option_type(ty: &Type) -> bool {
    quote!(#ty)
        .to_string()
        .replace(' ', "")
        .starts_with("Option<")
}

/// Parses `{ name: <expr>, ... }` for path or query values, checking each name
/// against the endpoint's parameters.
fn parse_param_values(
    input: syn::parse::ParseStream,
    known: &[String],
    kind: &str,
) -> syn::Result<Vec<(Ident, syn::Expr)>> {
    let body;
    syn::braced!(body in input);
    let mut values: Vec<(Ident, syn::Expr)> = Vec::new();
    while !body.is_empty() {
        let name = body.parse::<Ident>()?;
        let _ = body.parse::<Token![:]>()?;
        let value = body.parse::<syn::Expr>()?;
        if !known.contains(&name.to_string()) {
            return Err(syn::Error::new(
                name.span(),
                format!("this endpoint has no {kind} parameter `{name}`"),
            ));
        }
        if values.iter().any(|(existing, _)| existing == &name) {
            return Err(syn::Error::new(
                name.span(),
                format!("{kind} parameter `{name}` is given more than once"),
            ));
        }
        values.push((name, value));
        if body.peek(Token![,]) {
            let _ = body.parse::<Token![,]>()?;
        }
    }
    Ok(values)
}

/// Parses an HTTP status literal within `range`.
fn parse_status(
    input: syn::parse::ParseStream,
    range: std::ops::RangeInclusive<u16>,
    what: &str,
) -> syn::Result<u16> {
    let literal = input.parse::<syn::LitInt>()?;
    let status = literal.base10_parse::<u16>()?;
    if !range.contains(&status) {
        return Err(syn::Error::new(
            literal.span(),
            format!(
                "{what} status must be in {}..={}",
                range.start(),
                range.end()
            ),
        ));
    }
    Ok(status)
}

/// Parses `error: { status: 404, message: "..." }`.
fn parse_endpoint_error(input: syn::parse::ParseStream) -> syn::Result<(u16, String)> {
    let body;
    let braces = syn::braced!(body in input);
    let mut status = None;
    let mut message = None;
    while !body.is_empty() {
        let field = body.parse::<Ident>()?;
        let _ = body.parse::<Token![:]>()?;
        match field.to_string().as_str() {
            "status" => set_once(
                &mut status,
                &field,
                parse_status(&body, 400..=599, "an error")?,
            )?,
            "message" => set_once(&mut message, &field, body.parse::<LitStr>()?.value())?,
            _ => return Err(syn::Error::new(field.span(), "Expected status or message")),
        }
        if body.peek(Token![,]) {
            let _ = body.parse::<Token![,]>()?;
        }
    }
    Ok((
        status.ok_or_else(|| {
            syn::Error::new(braces.span.join(), "an example error needs a `status`")
        })?,
        message.ok_or_else(|| {
            syn::Error::new(braces.span.join(), "an example error needs a `message`")
        })?,
    ))
}

/// Parses `examples: [ name { path: {..}, query: {..}, body: .., response: .. }, ... ]`.
fn parse_endpoint_examples(
    input: syn::parse::ParseStream,
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
    response_type: &Type,
) -> syn::Result<Vec<EndpointExample>> {
    let path_names: Vec<String> = path_params.iter().map(|p| p.name.to_string()).collect();
    let query_names: Vec<String> = query_params.iter().map(|p| p.name.to_string()).collect();

    let list;
    syn::bracketed!(list in input);
    let mut examples = Vec::new();
    let mut names = std::collections::HashSet::new();
    while !list.is_empty() {
        let docs = parse_doc_comment_attrs(list.call(syn::Attribute::parse_outer)?, "example")?;
        let name = list.parse::<Ident>()?;
        if !names.insert(name.to_string()) {
            return Err(syn::Error::new(
                name.span(),
                format!("duplicate example name `{name}`"),
            ));
        }

        let body_content;
        syn::braced!(body_content in list);
        let mut path = None;
        let mut query = None;
        let mut body = None;
        let mut status = None;
        let mut response = None;
        let mut error = None;
        while !body_content.is_empty() {
            let field = body_content.parse::<Ident>()?;
            let _ = body_content.parse::<Token![:]>()?;
            match field.to_string().as_str() {
                "path" => set_once(
                    &mut path,
                    &field,
                    parse_param_values(&body_content, &path_names, "path")?,
                )?,
                "query" => set_once(
                    &mut query,
                    &field,
                    parse_param_values(&body_content, &query_names, "query")?,
                )?,
                "body" => set_once(&mut body, &field, body_content.parse::<syn::Expr>()?)?,
                "status" => set_once(
                    &mut status,
                    &field,
                    parse_status(&body_content, 200..=299, "a success")?,
                )?,
                "response" => set_once(&mut response, &field, body_content.parse::<syn::Expr>()?)?,
                "error" => set_once(&mut error, &field, parse_endpoint_error(&body_content)?)?,
                _ => {
                    return Err(syn::Error::new(
                        field.span(),
                        "Expected path, query, body, status, response, or error",
                    ));
                }
            }
            if body_content.peek(Token![,]) {
                let _ = body_content.parse::<Token![,]>()?;
            }
        }

        let path = path.unwrap_or_default();
        if let Some(missing) = path_names
            .iter()
            .find(|param| !path.iter().any(|(given, _)| given == *param))
        {
            return Err(syn::Error::new(
                name.span(),
                format!("example `{name}` needs a value for path parameter `{missing}`"),
            ));
        }
        let query = query.unwrap_or_default();
        if let Some(missing) = query_params.iter().find(|param| {
            !is_option_type(&param.param_type)
                && !query.iter().any(|(given, _)| given == &param.name)
        }) {
            return Err(syn::Error::new(
                name.span(),
                format!(
                    "example `{name}` needs a value for query parameter `{}`, which is not optional",
                    missing.name
                ),
            ));
        }
        match (&body, request_type) {
            (None, Some(_)) => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("example `{name}` needs a `body`, a value of the request type"),
                ));
            }
            (Some(body), None) => {
                return Err(syn::Error::new_spanned(
                    body,
                    "this endpoint takes no request body; leave `body` out",
                ));
            }
            _ => {}
        }

        let outcome = match (error, status, response) {
            (Some((status, message)), None, None) => {
                EndpointExampleOutcome::Error { status, message }
            }
            (Some(_), _, _) => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("example `{name}` has an `error`; leave out `status` and `response`"),
                ));
            }
            (None, status, response) => {
                let status = status.unwrap_or(200);
                if status == 204 {
                    if let Some(response) = response {
                        return Err(syn::Error::new_spanned(
                            response,
                            "a 204 response has no body; leave `response` out",
                        ));
                    }
                } else if response.is_none() && !is_unit_type(response_type) {
                    return Err(syn::Error::new(
                        name.span(),
                        format!("example `{name}` needs a `response` or an `error`"),
                    ));
                }
                EndpointExampleOutcome::Success { status, response }
            }
        };

        examples.push(EndpointExample {
            name,
            docs,
            path,
            query,
            body,
            outcome,
        });
        if list.peek(Token![,]) {
            let _ = list.parse::<Token![,]>()?;
        }
    }
    Ok(examples)
}

#[cfg(test)]
mod example_diagnostics {
    use super::*;

    fn parse(endpoints: &str) -> syn::Result<ServiceDefinition> {
        syn::parse_str(&format!(
            "{{ service_name: S, base_path: \"/api\", openapi: true, endpoints: [ {endpoints} ] }}"
        ))
    }

    #[test]
    fn a_well_formed_example_list_parses() {
        let service = parse(
            r#"
            POST UNAUTHORIZED items/{id: u32} ? notify: Option<bool> & tag: String (Body) -> Item {
                examples: [
                    /// Docs.
                    ok { path: { id: 1 }, query: { tag: "a".into() }, body: make(), response: item() },
                    created { path: { id: 2 }, query: { tag: "b".into(), notify: Some(true) }, body: make(), status: 201, response: item() },
                    missing { path: { id: 3 }, query: { tag: "c".into() }, body: make(), error: { status: 404, message: "gone" } },
                ],
            }
            "#,
        )
        .unwrap();
        let examples = &service.endpoints[0].examples;
        assert_eq!(examples.len(), 3);
        assert!(matches!(
            examples[1].outcome,
            EndpointExampleOutcome::Success { status: 201, .. }
        ));
        assert!(matches!(
            examples[2].outcome,
            EndpointExampleOutcome::Error { status: 404, .. }
        ));
    }

    #[test]
    fn diagnostics() {
        let cases = [
            (
                "GET UNAUTHORIZED items/{id: u32} () -> Item { examples: [ a { response: 1 } ] }",
                "needs a value for path parameter `id`",
            ),
            (
                "GET UNAUTHORIZED items/{id: u32} () -> Item { examples: [ a { path: { id: 1, other: 2 }, response: 1 } ] }",
                "no path parameter `other`",
            ),
            (
                "GET UNAUTHORIZED search ? q: String () -> Item { examples: [ a { response: 1 } ] }",
                "query parameter `q`, which is not optional",
            ),
            (
                "POST UNAUTHORIZED items (Body) -> Item { examples: [ a { response: 1 } ] }",
                "needs a `body`",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { body: 1, response: 1 } ] }",
                "takes no request body",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { } ] }",
                "needs a `response` or an `error`",
            ),
            (
                "DELETE UNAUTHORIZED items () -> () { examples: [ a { status: 204, response: () } ] }",
                "a 204 response has no body",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { status: 404, response: 1 } ] }",
                "a success status must be in 200..=299",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { error: { status: 200, message: \"x\" } } ] }",
                "an error status must be in 400..=599",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { response: 1, error: { status: 404, message: \"x\" } } ] }",
                "leave out `status` and `response`",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { response: 1 }, a { response: 2 } ] }",
                "duplicate example name `a`",
            ),
            (
                "GET UNAUTHORIZED items () -> Item { examples: [ a { error: { message: \"x\" } } ] }",
                "needs a `status`",
            ),
            (
                "GET UNAUTHORIZED v2/items () -> Item { versions: [ v1 { path: v1/items, response: Item, migration: M, examples: [] } ] }",
                "examples belong on the current endpoint only",
            ),
        ];
        for (input, expected) in cases {
            let message = parse(input).expect_err(input).to_string();
            assert!(message.contains(expected), "{input}\n  gave: {message}");
        }
        // A `()` response may leave `response` out.
        parse("DELETE UNAUTHORIZED items () -> () { examples: [ a { } ] }").unwrap();
    }
}
