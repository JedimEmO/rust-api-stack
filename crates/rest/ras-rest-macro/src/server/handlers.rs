//! Canonical handler invocation and response handling.

use super::request::{effective_body_limit_tokens, generate_body_extraction};
use crate::ast::*;
use quote::quote;
use syn::Ident;

pub(super) fn generate_handler_body(
    endpoint: &EndpointDefinition,
    handler_name: &Ident,
    method: &str,
    path: &str,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let body_limit_tokens = effective_body_limit_tokens(endpoint);
    match &endpoint.auth {
        AuthRequirement::Unauthorized => {
            let mut args = Vec::new();

            // Opt-in request headers (before path params)
            if endpoint.with_headers {
                args.push(quote! { headers.clone() });
            }

            if endpoint.path_params.len() == 1 {
                args.push(quote! { path_params });
            } else {
                for (i, _) in endpoint.path_params.iter().enumerate() {
                    let idx = syn::Index::from(i);
                    args.push(quote! { path_params.#idx });
                }
            }

            for query_param in &endpoint.query_params {
                let param_name = &query_param.name;
                args.push(quote! { query_params.#param_name });
            }

            let json_handling = if endpoint.request_type.is_some() {
                args.push(quote! { body });
                generate_body_extraction(require_json, &body_limit_tokens, method, path)
            } else {
                quote! {}
            };

            quote! {
                #json_handling

                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, None, #method, #path).await;
                }

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#args),*).await {
                    Ok(rest_response) => {
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        __ras_success_response(status_code, rest_response.body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, None, duration).await;
                }

                result
            }
        }
        AuthRequirement::OptionalAuth => {
            // Build argument list; the caller is passed by value as the first arg.
            let mut args = vec![quote! { caller }];

            // Opt-in request headers (after the caller, before path params)
            if endpoint.with_headers {
                args.push(quote! { headers.clone() });
            }

            if endpoint.path_params.len() == 1 {
                args.push(quote! { path_params });
            } else {
                for (i, _) in endpoint.path_params.iter().enumerate() {
                    let idx = syn::Index::from(i);
                    args.push(quote! { path_params.#idx });
                }
            }

            for query_param in &endpoint.query_params {
                let param_name = &query_param.name;
                args.push(quote! { query_params.#param_name });
            }

            let json_handling = if endpoint.request_type.is_some() {
                args.push(quote! { body });
                generate_body_extraction(require_json, &body_limit_tokens, method, path)
            } else {
                quote! {}
            };

            quote! {
                // Best-effort authentication for an OPTIONAL_AUTH route: never
                // rejected — Caller::Anonymous when no/invalid credential is
                // present, Caller::Authenticated otherwise.
                let caller = ras_auth_core::resolve_caller(
                    #method,
                    &headers,
                    &auth_transport,
                    auth_provider.as_deref(),
                ).await;
                // Snapshot the user for tracking; `caller` is moved into the handler.
                let __ras_caller_user = caller.authenticated().cloned();

                #json_handling

                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, __ras_caller_user.as_ref(), #method, #path).await;
                }

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#args),*).await {
                    Ok(rest_response) => {
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        __ras_success_response(status_code, rest_response.body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, __ras_caller_user.as_ref(), duration).await;
                }

                result
            }
        }
        AuthRequirement::WithPermissions(_) => {
            let mut args = vec![quote! { &user }];

            // Opt-in request headers (after the user, before path params)
            if endpoint.with_headers {
                args.push(quote! { headers.clone() });
            }

            if endpoint.path_params.len() == 1 {
                args.push(quote! { path_params });
            } else {
                for (i, _) in endpoint.path_params.iter().enumerate() {
                    let idx = syn::Index::from(i);
                    args.push(quote! { path_params.#idx });
                }
            }

            for query_param in &endpoint.query_params {
                let param_name = &query_param.name;
                args.push(quote! { query_params.#param_name });
            }

            let json_handling = if endpoint.request_type.is_some() {
                args.push(quote! { body });
                generate_body_extraction(require_json, &body_limit_tokens, method, path)
            } else {
                quote! {}
            };

            quote! {
                // Authenticate and authorize: credential → CSRF → authenticate
                // → OR-of-AND permission groups (shared ras-auth-core pipeline)
                let user = match ras_auth_core::authorize_request(
                    #method,
                    &headers,
                    &auth_transport,
                    auth_provider.as_deref(),
                    &required_permission_groups,
                ).await {
                    Ok(user) => user,
                    Err(error) => return __ras_authorize_error_response(error),
                };

                // Read and parse the body only after auth has succeeded
                #json_handling

                if let Some(tracker) = &with_usage_tracker {
                    let tracker_headers =
                        ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                    tracker(&tracker_headers, Some(&user), #method, #path).await;
                }

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#args),*).await {
                    Ok(rest_response) => {
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        __ras_success_response(status_code, rest_response.body)
                    },
                    Err(rest_error) => {
                        use axum::response::IntoResponse;

                        if let Some(internal) = &rest_error.internal_error {
                            ras_rest_core::tracing::error!(error = ?internal, "Request failed with status {}", rest_error.status);
                        }

                        let status_code = axum::http::StatusCode::from_u16(rest_error.status)
                            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

                        (
                            status_code,
                            axum::Json(serde_json::json!({
                                "error": &rest_error.message
                            }))
                        ).into_response()
                    },
                };

                let duration = start_time.elapsed();
                if let Some(tracker) = &with_method_duration_tracker {
                    tracker(#method, #path, Some(&user), duration).await;
                }

                result
            }
        }
    }
}
