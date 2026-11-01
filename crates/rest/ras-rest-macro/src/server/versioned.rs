//! Request and response adaptation for versioned routes.

use super::handlers::tracked_handler;
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

    // The handler takes the caller or user first: [caller/user?, headers, path.., query.., body?].
    match &endpoint.auth {
        AuthRequirement::Unauthorized => {}
        AuthRequirement::OptionalAuth => canonical_args.insert(0, quote! { caller }),
        AuthRequirement::WithPermissions(_) => canonical_args.insert(0, quote! { &user }),
    }

    let tracked = tracked_handler(
        method,
        path,
        quote! {
            #json_handling

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

            match service.#handler_name(#(#canonical_args),*).await {
                Ok(rest_response) => {
                    use axum::response::IntoResponse;
                    let status_code = axum::http::StatusCode::from_u16(rest_response.status)
                        .unwrap_or(axum::http::StatusCode::OK);
                    match <#migration_type as ras_rest_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(rest_response.body) {
                        Ok(body) => __ras_success_response(status_code, body),
                        Err(e) => {
                            ras_rest_core::tracing::error!(error = %e, "Response migration failed");
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                axum::Json(serde_json::json!({
                                    "error": "Internal server error"
                                }))
                            ).into_response()
                        },
                    }
                },
                Err(rest_error) => __ras_rest_error_response(rest_error),
            }
        },
    );

    match &endpoint.auth {
        AuthRequirement::Unauthorized => quote! {
            let __ras_tracker_user: Option<&ras_auth_core::AuthenticatedUser> = None;
            #tracked
        },
        AuthRequirement::OptionalAuth => quote! {
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
            let __ras_tracker_user: Option<&ras_auth_core::AuthenticatedUser> = __ras_caller_user.as_ref();
            #tracked
        },
        AuthRequirement::WithPermissions(_) => quote! {
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

            // The body is read and parsed only after auth has succeeded.
            let __ras_tracker_user: Option<&ras_auth_core::AuthenticatedUser> = Some(&user);
            #tracked
        },
    }
}
