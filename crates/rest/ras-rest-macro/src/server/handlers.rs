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
            let tracked = tracked_handler(
                method,
                path,
                quote! {
                    #json_handling
                    match service.#handler_name(#(#args),*).await {
                        Ok(rest_response) => {
                            let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                                .unwrap_or(axum::http::StatusCode::OK);
                            __ras_success_response(status_code, rest_response.body)
                        },
                        Err(rest_error) => __ras_rest_error_response(rest_error),
                    }
                },
            );

            quote! {
                let __ras_tracker_user: Option<&ras_auth_core::AuthenticatedUser> = None;
                #tracked
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
            let tracked = tracked_handler(
                method,
                path,
                quote! {
                    #json_handling
                    match service.#handler_name(#(#args),*).await {
                        Ok(rest_response) => {
                            let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                                .unwrap_or(axum::http::StatusCode::OK);
                            __ras_success_response(status_code, rest_response.body)
                        },
                        Err(rest_error) => __ras_rest_error_response(rest_error),
                    }
                },
            );

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

                let __ras_tracker_user: Option<&ras_auth_core::AuthenticatedUser> = __ras_caller_user.as_ref();
                #tracked
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
            let tracked = tracked_handler(
                method,
                path,
                quote! {
                    #json_handling
                    match service.#handler_name(#(#args),*).await {
                        Ok(rest_response) => {
                            let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                                .unwrap_or(axum::http::StatusCode::OK);
                            __ras_success_response(status_code, rest_response.body)
                        },
                        Err(rest_error) => __ras_rest_error_response(rest_error),
                    }
                },
            );

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
                let __ras_tracker_user: Option<&ras_auth_core::AuthenticatedUser> = Some(&user);
                #tracked
            }
        }
    }
}

/// Wraps request decoding and the handler call in the usage and duration trackers.
///
/// Authorization runs before this code and may return early; such a rejection
/// reaches neither tracker. `body` runs inside an async block, so an early
/// `return` there (a body that does not decode, a failed migration) still
/// produces a response that the duration tracker records. The request counts as
/// a success unless the response status is 4xx or 5xx.
///
/// Expects `__ras_tracker_user: Option<&AuthenticatedUser>` in scope.
pub(super) fn tracked_handler(
    method: &str,
    path: &str,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        if let Some(tracker) = &with_usage_tracker {
            let tracker_headers =
                ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
            tracker(&tracker_headers, __ras_tracker_user, #method, #path).await;
        }

        let __ras_start = std::time::Instant::now();
        let __ras_response: axum::response::Response = async {
            #body
        }
        .await;

        if let Some(tracker) = &with_method_duration_tracker {
            let __ras_status = __ras_response.status();
            let __ras_success = !(__ras_status.is_client_error() || __ras_status.is_server_error());
            tracker(#method, #path, __ras_tracker_user, __ras_start.elapsed(), __ras_success).await;
        }

        __ras_response
    }
}
