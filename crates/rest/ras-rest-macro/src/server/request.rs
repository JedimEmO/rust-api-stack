//! Request-part models and fallible extraction.

use crate::ast::*;
use quote::quote;
use syn::{Ident, Type};

pub(super) fn pascal_ident_segment(value: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = true;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                out.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                out.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }

    if out.is_empty() {
        "Version".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("V{out}")
    } else {
        out
    }
}

pub(super) fn rest_request_part_idents(
    service_name: &Ident,
    handler_name: &Ident,
    version: &str,
) -> (Ident, Ident, Ident) {
    let service = service_name.to_string();
    let handler = pascal_ident_segment(&handler_name.to_string());
    let version = pascal_ident_segment(version);
    let request_ident = quote::format_ident!("{}{}{}Request", service, handler, version);
    let path_ident = quote::format_ident!("{}{}{}Path", service, handler, version);
    let query_ident = quote::format_ident!("{}{}{}Query", service, handler, version);
    (request_ident, path_ident, query_ident)
}

pub(super) fn rest_body_type_tokens(request_type: Option<&Type>) -> proc_macro2::TokenStream {
    match request_type {
        Some(request_type) => quote! { #request_type },
        None => quote! { () },
    }
}

pub(super) fn generate_rest_request_part_structs(
    service_def: &ServiceDefinition,
) -> proc_macro2::TokenStream {
    let structs = service_def.endpoints.iter().flat_map(|endpoint| {
        if endpoint.versions.is_empty() {
            return Vec::new();
        }

        let canonical_version = endpoint.version.as_deref().unwrap_or("current");
        let mut structs = vec![generate_rest_request_part_struct(
            &service_def.service_name,
            &endpoint.handler_name,
            canonical_version,
            &endpoint.path_params,
            &endpoint.query_params,
            endpoint.request_type.as_ref(),
        )];

        structs.extend(endpoint.versions.iter().map(|version| {
            generate_rest_request_part_struct(
                &service_def.service_name,
                &endpoint.handler_name,
                &version.version,
                &version.path_params,
                &version.query_params,
                version.request_type.as_ref(),
            )
        }));

        structs
    });

    quote! {
        #(#structs)*
    }
}

pub(super) fn generate_rest_request_part_struct(
    service_name: &Ident,
    handler_name: &Ident,
    version: &str,
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
) -> proc_macro2::TokenStream {
    let (request_ident, path_ident, query_ident) =
        rest_request_part_idents(service_name, handler_name, version);
    let path_fields = path_params.iter().map(|param| {
        let name = &param.name;
        let param_type = &param.param_type;
        quote! { pub #name: #param_type }
    });
    let query_fields = query_params.iter().map(|param| {
        let name = &param.name;
        let param_type = &param.param_type;
        quote! { pub #name: #param_type }
    });
    let body_type = rest_body_type_tokens(request_type);

    quote! {
        pub struct #path_ident {
            #(#path_fields),*
        }

        pub struct #query_ident {
            #(#query_fields),*
        }

        pub struct #request_ident {
            pub path: #path_ident,
            pub query: #query_ident,
            pub body: #body_type,
        }
    }
}

pub(super) fn generate_rest_parts_init(
    service_name: &Ident,
    handler_name: &Ident,
    version: &str,
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
) -> proc_macro2::TokenStream {
    let (request_ident, path_ident, query_ident) =
        rest_request_part_idents(service_name, handler_name, version);

    let path_values = path_params.iter().enumerate().map(|(idx, param)| {
        let name = &param.name;
        if path_params.len() == 1 {
            quote! { #name: path_params }
        } else {
            let idx = syn::Index::from(idx);
            quote! { #name: path_params.#idx }
        }
    });

    let query_values = query_params.iter().map(|param| {
        let name = &param.name;
        quote! { #name: query_params.#name }
    });

    let body_value = if request_type.is_some() {
        quote! { body }
    } else {
        quote! { () }
    };

    quote! {
        #request_ident {
            path: #path_ident {
                #(#path_values),*
            },
            query: #query_ident {
                #(#query_values),*
            },
            body: #body_value,
        }
    }
}

pub(super) fn rest_canonical_args_from_parts(
    endpoint: &EndpointDefinition,
    parts_ident: &Ident,
) -> Vec<proc_macro2::TokenStream> {
    let mut args = Vec::new();

    for path_param in &endpoint.path_params {
        let name = &path_param.name;
        args.push(quote! { #parts_ident.path.#name });
    }

    for query_param in &endpoint.query_params {
        let name = &query_param.name;
        args.push(quote! { #parts_ident.query.#name });
    }

    if endpoint.request_type.is_some() {
        args.push(quote! { #parts_ident.body });
    }

    args
}

pub(super) fn generate_query_struct(
    struct_name: &Ident,
    query_params: &[QueryParam],
) -> proc_macro2::TokenStream {
    if query_params.is_empty() {
        return quote! {};
    }

    let fields = query_params.iter().map(|param| {
        let name = &param.name;
        let param_type = &param.param_type;
        quote! { pub #name: #param_type }
    });

    quote! {
        #[derive(serde::Deserialize)]
        pub(super) struct #struct_name {
            #(#fields),*
        }
    }
}

/// Generated handler signature plus the prelude that unwraps fallible
/// extractors inside the handler body.
pub(super) struct AxumHandlerParts {
    /// Closure parameter list (`headers`, path/query extractors, raw request).
    pub(super) extractors: proc_macro2::TokenStream,
    /// Emitted at the top of the handler body. Path and query extractors are
    /// taken as `Result<_, Rejection>` so the axum default rejection body —
    /// which echoes the offending value verbatim, e.g.
    /// ``Invalid URL: Cannot parse `abc` to a `i32` `` — is never sent to the
    /// client. The detail is logged at `warn` and a fixed message is returned.
    pub(super) prelude: proc_macro2::TokenStream,
}

pub(super) fn generate_axum_handler(
    path_params: &[PathParam],
    query_params: &[QueryParam],
    request_type: Option<&Type>,
    query_struct_name: &Ident,
    method: &str,
    path: &str,
) -> AxumHandlerParts {
    let mut extractors = Vec::new();
    let mut prelude = Vec::new();

    extractors.push(quote! { headers: axum::http::HeaderMap });

    if !path_params.is_empty() {
        let path_param_types = path_params.iter().map(|param| &param.param_type);
        let path_ty = if path_params.len() == 1 {
            quote! { axum::extract::Path<#(#path_param_types)*> }
        } else {
            quote! { axum::extract::Path<(#(#path_param_types),*)> }
        };
        extractors.push(quote! {
            __ras_path: Result<#path_ty, axum::extract::rejection::PathRejection>
        });
        prelude.push(quote! {
            let axum::extract::Path(path_params) = match __ras_path {
                Ok(path) => path,
                Err(__ras_rejection) => {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        status = __ras_rejection.status().as_u16(),
                        detail = %ras_rest_core::sanitize_log_detail(&__ras_rejection.body_text()),
                        "rejected request: invalid path parameters"
                    );
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": "Invalid path parameters" }))
                    ).into_response();
                }
            };
        });
    }

    if !query_params.is_empty() {
        extractors.push(quote! {
            __ras_query: Result<
                ::axum_extra::extract::Query<query_params::#query_struct_name>,
                ::axum_extra::extract::QueryRejection,
            >
        });
        prelude.push(quote! {
            let ::axum_extra::extract::Query(query_params) = match __ras_query {
                Ok(query) => query,
                Err(__ras_rejection) => {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        status = __ras_rejection.status().as_u16(),
                        detail = %ras_rest_core::sanitize_log_detail(&__ras_rejection.body_text()),
                        "rejected request: invalid query parameters"
                    );
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({ "error": "Invalid query parameters" }))
                    ).into_response();
                }
            };
        });
    }

    // Take the raw request when a body is declared. The body is read and
    // deserialized inside the handler AFTER auth/CSRF/permission checks, so
    // unauthenticated clients cannot make the server buffer or parse payloads.
    if request_type.is_some() {
        extractors.push(quote! { request: axum::extract::Request });
    }

    AxumHandlerParts {
        extractors: quote! { #(#extractors),* },
        prelude: quote! { #(#prelude)* },
    }
}

/// Generated code that reads and JSON-deserializes the request body from the
/// raw `request` extractor, bounded by `limit`.
///
/// For authenticated endpoints this must be emitted AFTER the
/// auth/CSRF/permission block so unauthenticated clients cannot make the
/// server buffer or parse payloads.
///
/// Behavior:
/// * When `require_json` is set, a request whose `Content-Type` is not
///   `application/json` (ignoring parameters like `; charset=utf-8`) is rejected
///   with `415 Unsupported Media Type` before the body is read. Requiring
///   `application/json` also forces a CORS preflight for cross-origin requests,
///   which no CORS layer answers by default — defense-in-depth against
///   simple-request CSRF on cookie-authenticated endpoints.
/// * A declared `Content-Length` over `limit` is rejected with `413` up front so
///   a subsequent `to_bytes` error is unambiguously a read failure (`400`),
///   rather than the two being conflated as "too large".
/// * A malformed JSON body is logged (category + line/column, never the rejected
///   value) at `warn` before returning `400`, matching the handler-error logging
///   convention.
pub(super) fn generate_body_extraction(
    require_json: bool,
    limit: &proc_macro2::TokenStream,
    method: &str,
    path: &str,
) -> proc_macro2::TokenStream {
    let content_type_check = if require_json {
        quote! {
            {
                let __ras_content_type_ok = headers
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| {
                        value
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .eq_ignore_ascii_case("application/json")
                    })
                    .unwrap_or(false);
                if !__ras_content_type_ok {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        "rejected request: Content-Type is not application/json"
                    );
                    return (
                        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        axum::Json(serde_json::json!({
                            "error": "Unsupported Media Type: expected application/json"
                        }))
                    ).into_response();
                }
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #content_type_check

        let body = {
            // Reject an over-declared Content-Length up front so a 413 is
            // unambiguous without reading the body. A chunked body with no
            // declared length is still capped by `to_bytes`; that error is then
            // classified below (over-limit -> 413, genuine read error -> 400).
            if let Some(__ras_declared_len) = headers
                .get(axum::http::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
            {
                if __ras_declared_len > #limit {
                    use axum::response::IntoResponse;
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        declared_len = __ras_declared_len,
                        limit = #limit,
                        "rejected request: body exceeds limit"
                    );
                    return (
                        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                        axum::Json(serde_json::json!({
                            "error": "Request body too large"
                        }))
                    ).into_response();
                }
            }

            let body_bytes = match ::axum::body::to_bytes(request.into_body(), #limit).await {
                Ok(bytes) => bytes,
                Err(__ras_body_err) => {
                    use axum::response::IntoResponse;
                    // `to_bytes` fails for both an over-limit body and a genuine
                    // stream read error. axum wraps http_body_util's
                    // `LengthLimitError` (Display: "length limit exceeded") for
                    // the former; classify on it so a read failure is a 400 and
                    // only a real overflow is a 413.
                    let (__ras_status, __ras_client_msg) =
                        if __ras_body_err.to_string().contains("length limit exceeded") {
                            (
                                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                                "Request body too large",
                            )
                        } else {
                            (
                                axum::http::StatusCode::BAD_REQUEST,
                                "Could not read request body",
                            )
                        };
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        status = __ras_status.as_u16(),
                        "rejected request: {}",
                        __ras_client_msg
                    );
                    return (
                        __ras_status,
                        axum::Json(serde_json::json!({ "error": __ras_client_msg }))
                    ).into_response();
                },
            };
            match serde_json::from_slice(&body_bytes) {
                Ok(body) => body,
                Err(__ras_json_err) => {
                    use axum::response::IntoResponse;
                    // Log the classification and location so the reason is
                    // recoverable server-side; never log the rejected value.
                    ras_rest_core::tracing::warn!(
                        method = #method,
                        path = #path,
                        category = ?__ras_json_err.classify(),
                        line = __ras_json_err.line(),
                        column = __ras_json_err.column(),
                        "rejected request: malformed JSON body"
                    );
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({
                            "error": "Invalid JSON"
                        }))
                    ).into_response();
                },
            }
        };
    }
}

/// The effective body-size limit expression for an endpoint: its per-endpoint
/// `body_limit` override when set, otherwise the service-level `__RAS_BODY_LIMIT`.
pub(super) fn effective_body_limit_tokens(
    endpoint: &EndpointDefinition,
) -> proc_macro2::TokenStream {
    match endpoint.body_limit {
        Some(limit) => quote! { #limit },
        None => quote! { __RAS_BODY_LIMIT },
    }
}
