//! JSON-RPC service syntax and diagnostics.

use crate::ast::*;
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

        let mut openrpc = None;
        let mut explorer = None;
        let mut feature_gated = false;
        let mut require_json_content_type = true;
        let mut body_limit = None;
        let mut docs_require_auth = false;

        while content.peek(Ident) {
            let field_name = content.fork().parse::<Ident>()?;
            if field_name == "methods" {
                break;
            }

            let _ = content.parse::<Ident>()?; // field name
            let _ = content.parse::<Token![:]>()?;

            if field_name == "openrpc" {
                if content.peek(syn::LitBool) {
                    let enabled = content.parse::<syn::LitBool>()?;
                    if enabled.value() {
                        openrpc = Some(OpenRpcConfig::Enabled);
                    }
                } else if content.peek(syn::token::Brace) {
                    let openrpc_content;
                    syn::braced!(openrpc_content in content);

                    let _ = openrpc_content.parse::<Ident>()?; // "output"
                    let _ = openrpc_content.parse::<Token![:]>()?;
                    let path = openrpc_content.parse::<LitStr>()?;
                    openrpc = Some(OpenRpcConfig::WithPath(path.value()));
                }
            } else if field_name == "explorer" {
                if content.peek(syn::LitBool) {
                    let enabled = content.parse::<syn::LitBool>()?;
                    if enabled.value() {
                        explorer = Some(ExplorerConfig::Enabled);
                    }
                } else if content.peek(syn::token::Brace) {
                    let explorer_content;
                    syn::braced!(explorer_content in content);

                    let _ = explorer_content.parse::<Ident>()?; // "path"
                    let _ = explorer_content.parse::<Token![:]>()?;
                    let path = explorer_content.parse::<LitStr>()?;
                    explorer = Some(ExplorerConfig::WithPath(path.value()));
                }
            } else if field_name == "feature_gated" {
                let enabled = content.parse::<syn::LitBool>()?;
                feature_gated = enabled.value();
            } else if field_name == "require_json_content_type" {
                let enabled = content.parse::<syn::LitBool>()?;
                require_json_content_type = enabled.value();
            } else if field_name == "body_limit" {
                let limit = content.parse::<syn::LitInt>()?;
                body_limit = Some(limit.base10_parse::<usize>()?);
            } else if field_name == "docs_require_auth" {
                let enabled = content.parse::<syn::LitBool>()?;
                docs_require_auth = enabled.value();
            } else {
                return Err(syn::Error::new(
                    field_name.span(),
                    format!("Unknown field: {field_name}"),
                ));
            }

            let _ = content.parse::<Token![,]>()?;
        }

        let _ = content.parse::<Ident>()?; // "methods"
        let _ = content.parse::<Token![:]>()?;

        let methods_content;
        syn::bracketed!(methods_content in content);

        let mut methods = Vec::new();
        while !methods_content.is_empty() {
            let method = methods_content.parse::<MethodDefinition>()?;
            methods.push(method);

            if methods_content.peek(Token![,]) {
                let _ = methods_content.parse::<Token![,]>()?;
            }
        }

        Ok(ServiceDefinition {
            service_name,
            openrpc,
            explorer,
            feature_gated,
            require_json_content_type,
            body_limit,
            docs_require_auth,
            methods,
        })
    }
}

impl Parse for MethodDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let docs = parse_doc_comment_attrs(input.call(syn::Attribute::parse_outer)?, "method")?;

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

        let name = input.parse::<Ident>()?;

        let request_content;
        syn::parenthesized!(request_content in input);
        let request_type = request_content.parse::<Type>()?;

        let _ = input.parse::<Token![->]>()?;
        let response_type = input.parse::<Type>()?;

        let mut version = None;
        let mut wire_name = None;
        let mut versions = Vec::new();
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
                    "wire" => {
                        wire_name = Some(content.parse::<LitStr>()?.value());
                    }
                    "versions" => {
                        let versions_content;
                        syn::bracketed!(versions_content in content);

                        while !versions_content.is_empty() {
                            versions.push(versions_content.parse::<MethodVersionDefinition>()?);

                            if versions_content.peek(Token![,]) {
                                let _ = versions_content.parse::<Token![,]>()?;
                            }
                        }
                    }
                    "examples" => {
                        examples = parse_examples(&content, &request_type)?;
                    }
                    _ => {
                        return Err(syn::Error::new(
                            field_name.span(),
                            "Expected version, wire, versions, or examples",
                        ));
                    }
                }

                if content.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                }
            }
        }

        Ok(MethodDefinition {
            docs,
            auth,
            name,
            request_type,
            response_type,
            version,
            wire_name,
            versions,
            examples,
        })
    }
}

impl Parse for MethodVersionDefinition {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let version = parse_label(input)?;

        let content;
        syn::braced!(content in input);

        let mut wire_name = None;
        let mut request_type = None;
        let mut response_type = None;
        let mut migration_type = None;

        while !content.is_empty() {
            let field_name = content.parse::<Ident>()?;
            let _ = content.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "wire" => {
                    wire_name = Some(content.parse::<LitStr>()?.value());
                }
                "request" => {
                    request_type = Some(content.parse::<Type>()?);
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
                        "examples belong on the current method only; \
                         legacy versions are documented without examples",
                    ));
                }
                _ => {
                    return Err(syn::Error::new(
                        field_name.span(),
                        "Expected wire, request, response, or migration",
                    ));
                }
            }

            if content.peek(Token![,]) {
                let _ = content.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            version,
            wire_name: wire_name
                .ok_or_else(|| syn::Error::new(input.span(), "Version entry is missing wire"))?,
            request_type: request_type
                .ok_or_else(|| syn::Error::new(input.span(), "Version entry is missing request"))?,
            response_type: response_type.ok_or_else(|| {
                syn::Error::new(input.span(), "Version entry is missing response")
            })?,
            migration_type: migration_type.ok_or_else(|| {
                syn::Error::new(input.span(), "Version entry is missing migration")
            })?,
        })
    }
}

fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
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

/// Parses `examples: [ name { params: ..., result: ... }, ... ]`.
fn parse_examples(
    input: syn::parse::ParseStream,
    request_type: &Type,
) -> syn::Result<Vec<MethodExample>> {
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

        let body;
        syn::braced!(body in list);
        let mut params = None;
        let mut result = None;
        let mut error = None;
        while !body.is_empty() {
            let field = body.parse::<Ident>()?;
            let _ = body.parse::<Token![:]>()?;
            match field.to_string().as_str() {
                "params" => set_once(&mut params, &field, body.parse::<syn::Expr>()?)?,
                "result" => set_once(&mut result, &field, body.parse::<syn::Expr>()?)?,
                "error" => set_once(&mut error, &field, parse_example_error(&body)?)?,
                _ => {
                    return Err(syn::Error::new(
                        field.span(),
                        "Expected params, result, or error",
                    ));
                }
            }
            if body.peek(Token![,]) {
                let _ = body.parse::<Token![,]>()?;
            }
        }

        match (&params, is_unit_type(request_type)) {
            (None, false) => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("example `{name}` needs `params`, a value of the request type"),
                ));
            }
            (Some(params), true) => {
                return Err(syn::Error::new_spanned(
                    params,
                    "this method takes no params (its request type is `()`); leave `params` out",
                ));
            }
            _ => {}
        }

        let outcome = match (result, error) {
            (Some(result), None) => ExampleOutcome::Result(result),
            (None, Some(error)) => error,
            (Some(_), Some(_)) => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("example `{name}` has both `result` and `error`; give one"),
                ));
            }
            (None, None) => {
                return Err(syn::Error::new(
                    name.span(),
                    format!("example `{name}` needs a `result` or an `error`"),
                ));
            }
        };

        examples.push(MethodExample {
            name,
            docs,
            params,
            outcome,
        });

        if list.peek(Token![,]) {
            let _ = list.parse::<Token![,]>()?;
        }
    }
    Ok(examples)
}

/// Parses `error: { code: 1001, message: "...", data: <expr> }`.
fn parse_example_error(input: syn::parse::ParseStream) -> syn::Result<ExampleOutcome> {
    let body;
    let braces = syn::braced!(body in input);

    let mut code = None;
    let mut message = None;
    let mut data = None;
    while !body.is_empty() {
        let field = body.parse::<Ident>()?;
        let _ = body.parse::<Token![:]>()?;
        match field.to_string().as_str() {
            "code" => set_once(&mut code, &field, parse_error_code(&body)?)?,
            "message" => set_once(&mut message, &field, body.parse::<LitStr>()?.value())?,
            "data" => set_once(&mut data, &field, body.parse::<syn::Expr>()?)?,
            _ => {
                return Err(syn::Error::new(
                    field.span(),
                    "Expected code, message, or data",
                ));
            }
        }
        if body.peek(Token![,]) {
            let _ = body.parse::<Token![,]>()?;
        }
    }

    Ok(ExampleOutcome::Error {
        code: code.ok_or_else(|| {
            syn::Error::new(braces.span.join(), "an example error needs a `code`")
        })?,
        message: message.ok_or_else(|| {
            syn::Error::new(braces.span.join(), "an example error needs a `message`")
        })?,
        data,
    })
}

/// Parses an integer literal, optionally negative, that a handler may send.
fn parse_error_code(input: syn::parse::ParseStream) -> syn::Result<i32> {
    let expr = input.parse::<syn::Expr>()?;
    let (negative, literal) = match &expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(literal),
            ..
        }) => (false, literal),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr: inner,
            ..
        }) => match inner.as_ref() {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(literal),
                ..
            }) => (true, literal),
            _ => return Err(syn::Error::new_spanned(expr, "expected an integer literal")),
        },
        _ => return Err(syn::Error::new_spanned(expr, "expected an integer literal")),
    };
    let magnitude = literal.base10_parse::<i64>()?;
    let code = i32::try_from(if negative { -magnitude } else { magnitude })
        .map_err(|_| syn::Error::new_spanned(&expr, "the code does not fit in an i32"))?;
    if !ras_jsonrpc_types::handler_code_allowed(code) {
        return Err(syn::Error::new_spanned(
            &expr,
            format!(
                "code {code} is reserved, and a handler cannot send it; use a code outside \
                 -32768..=-32000, or -32602 for invalid params"
            ),
        ));
    }
    Ok(code)
}

#[cfg(test)]
mod example_diagnostics {
    use super::*;

    fn parse(methods: &str) -> syn::Result<ServiceDefinition> {
        syn::parse_str(&format!(
            "{{ service_name: S, openrpc: true, methods: [ {methods} ] }}"
        ))
    }

    fn error(methods: &str) -> String {
        parse(methods).expect_err("should not parse").to_string()
    }

    #[test]
    fn a_well_formed_example_list_parses() {
        let service = parse(
            r#"
            UNAUTHORIZED create(Req) -> Resp {
                examples: [
                    /// Docs.
                    ok { params: Req { a: 1 }, result: Resp::default() },
                    taken { params: make(), error: { code: -32602, message: "bad", data: 1 } },
                ],
            }
            "#,
        )
        .unwrap();
        let examples = &service.methods[0].examples;
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].docs.as_ref().unwrap().summary, "Docs.");
        assert!(matches!(
            examples[1].outcome,
            ExampleOutcome::Error { code: -32602, .. }
        ));
    }

    #[test]
    fn diagnostics() {
        let cases = [
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { result: 1 } ] }",
                "example `a` needs `params`",
            ),
            (
                "UNAUTHORIZED m(()) -> Resp { examples: [ a { params: 1, result: 1 } ] }",
                "takes no params",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1 } ] }",
                "needs a `result` or an `error`",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, result: 1, error: { code: 1, message: \"x\" } } ] }",
                "both `result` and `error`",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, result: 1 }, a { params: 2, result: 2 } ] }",
                "duplicate example name `a`",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, params: 2, result: 1 } ] }",
                "`params` is given more than once",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, outcome: 1 } ] }",
                "Expected params, result, or error",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, error: { code: -32001, message: \"x\" } } ] }",
                "code -32001 is reserved",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, error: { code: 99999999999, message: \"x\" } } ] }",
                "does not fit in an i32",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, error: { message: \"x\" } } ] }",
                "needs a `code`",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { examples: [ a { params: 1, error: { code: n, message: \"x\" } } ] }",
                "expected an integer literal",
            ),
            (
                "UNAUTHORIZED m(Req) -> Resp { versions: [ v1 { wire: \"m.v1\", request: A, response: B, migration: C, examples: [] } ] }",
                "examples belong on the current method only",
            ),
        ];
        for (input, expected) in cases {
            let message = error(input);
            assert!(message.contains(expected), "{input}\n  gave: {message}");
        }
    }
}
