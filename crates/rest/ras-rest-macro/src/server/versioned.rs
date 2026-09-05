//! Request and response adaptation for versioned routes.

use super::request::{
    effective_body_limit_tokens, generate_body_extraction, generate_rest_parts_init,
    rest_canonical_args_from_parts, rest_request_part_idents,
};
use crate::ast::*;
use quote::quote;
use syn::Ident;

pub(super) fn generate_legacy_handler_body(
    service_name: &Ident,
    endpoint: &EndpointDefinition,
    version: &EndpointVersionDefinition,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let handler_name = &endpoint.handler_name;
    let method = endpoint.method.as_str();
    let path = &version.path;
    let body_limit_tokens = effective_body_limit_tokens(endpoint);
    let migration_type = &version.migration_type;
    let canonical_response_type = &endpoint.response_type;
    let legacy_response_type = &version.response_type;
    let canonical_version = endpoint.version.as_deref().unwrap_or("current");
    let (canonical_request_ident, _, _) =
        rest_request_part_idents(service_name, handler_name, canonical_version);
    let (legacy_request_ident, _, _) =
        rest_request_part_idents(service_name, handler_name, &version.version);
    let legacy_parts_init = generate_rest_parts_init(
        service_name,
        handler_name,
        &version.version,
        &version.path_params,
        &version.query_params,
        version.request_type.as_ref(),
    );
    let canonical_parts_ident = quote::format_ident!("canonical_parts");
    let mut canonical_args = rest_canonical_args_from_parts(endpoint, &canonical_parts_ident);

    // Opt-in request headers, inserted before the auth arg is prepended below so
    // the final order is [caller/user?, headers, path.., query.., body?].
    if endpoint.with_headers {
        canonical_args.insert(0, quote! { headers.clone() });
    }

    let json_handling = if version.request_type.is_some() {
        generate_body_extraction(require_json, &body_limit_tokens, method, path)
    } else {
        quote! {}
    };

    match &endpoint.auth {
        AuthRequirement::Unauthorized => quote! {
            #json_handling

            if let Some(tracker) = &with_usage_tracker {
                let tracker_headers =
                    ras_auth_core::redact_sensitive_headers_for_auth_transport(&headers, &auth_transport);
                tracker(&tracker_headers, None, #method, #path).await;
            }

            let legacy_parts: #legacy_request_ident = #legacy_parts_init;
            let #canonical_parts_ident: #canonical_request_ident =
                match <#migration_type as ras_rest_core::VersionMigration<#legacy_request_ident, #canonical_request_ident>>::migrate(legacy_parts) {
                    Ok(parts) => parts,
                    Err(e) => {
                        use axum::response::IntoResponse;
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({
                                "error": e.to_string()
                            }))
                        ).into_response();
                    },
                };

            let start_time = std::time::Instant::now();

            let result = match service.#handler_name(#(#canonical_args),*).await {
                Ok(rest_response) => {
                    use axum::response::IntoResponse;
                    let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                        .unwrap_or(axum::http::StatusCode::OK);
                    let body: #legacy_response_type =
                        match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                            Ok(body) => body,
                            Err(e) => {
                                ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                                return (
                                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                    axum::Json(serde_json::json!({
                                        "error": "Internal server error"
                                    }))
                                ).into_response();
                            },
                        };
                    __ras_success_response(status_code, body)
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
        },
        AuthRequirement::OptionalAuth => {
            canonical_args.insert(0, quote! { caller });

            quote! {
                // Best-effort authentication for an OPTIONAL_AUTH route — never
                // rejected: resolves to Caller::Anonymous for a missing/invalid
                // credential, Caller::Authenticated for a valid one.
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

                let legacy_parts: #legacy_request_ident = #legacy_parts_init;
                let #canonical_parts_ident: #canonical_request_ident =
                    match <#migration_type as ras_rest_core::VersionMigration<#legacy_request_ident, #canonical_request_ident>>::migrate(legacy_parts) {
                        Ok(parts) => parts,
                        Err(e) => {
                            use axum::response::IntoResponse;
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                axum::Json(serde_json::json!({
                                    "error": e.to_string()
                                }))
                            ).into_response();
                        },
                    };

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#canonical_args),*).await {
                    Ok(rest_response) => {
                        use axum::response::IntoResponse;
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        let body: #legacy_response_type =
                            match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                                Ok(body) => body,
                                Err(e) => {
                                    ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                                    return (
                                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                        axum::Json(serde_json::json!({
                                            "error": "Internal server error"
                                        }))
                                    ).into_response();
                                },
                            };
                        __ras_success_response(status_code, body)
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
            canonical_args.insert(0, quote! { &user });

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

                let legacy_parts: #legacy_request_ident = #legacy_parts_init;
                let #canonical_parts_ident: #canonical_request_ident =
                    match <#migration_type as ras_rest_core::VersionMigration<#legacy_request_ident, #canonical_request_ident>>::migrate(legacy_parts) {
                        Ok(parts) => parts,
                        Err(e) => {
                            use axum::response::IntoResponse;
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                axum::Json(serde_json::json!({
                                    "error": e.to_string()
                                }))
                            ).into_response();
                        },
                    };

                let start_time = std::time::Instant::now();

                let result = match service.#handler_name(#(#canonical_args),*).await {
                    Ok(rest_response) => {
                        use axum::response::IntoResponse;
                        let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                            .unwrap_or(axum::http::StatusCode::OK);
                        let body: #legacy_response_type =
                            match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                                Ok(body) => body,
                                Err(e) => {
                                    ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                                    return (
                                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                        axum::Json(serde_json::json!({
                                            "error": "Internal server error"
                                        }))
                                    ).into_response();
                                },
                            };
                        __ras_success_response(status_code, body)
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
