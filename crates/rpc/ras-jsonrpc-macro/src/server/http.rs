//! HTTP routing, envelope handling, and request-level authentication.

use super::dispatch::{generate_jsonrpc_method_dispatches, jsonrpc_method_wire_name};
use crate::ast::*;
use quote::quote;

pub(super) fn generate_http_methods(service_def: &ServiceDefinition) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let service_name_str = service_name.to_string();

    let explorer_enabled = service_def.explorer.is_some() && service_def.openrpc.is_some();

    // Content-Type gate: reject a non-`application/json` body with 415 before
    // parsing. Requiring `application/json` forces a CORS preflight for
    // cross-origin requests, closing the simple-request CSRF shape.
    let content_type_gate = if service_def.require_json_content_type {
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
                    ras_jsonrpc_core::tracing::warn!(
                        "rejected JSON-RPC request: Content-Type is not application/json"
                    );
                    return (
                        axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        [("Content-Type", "application/json")],
                        serde_json::to_string(&ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::invalid_request(),
                            None,
                        ))
                        .unwrap_or_else(|_| "{}".to_string()),
                    );
                }
            }
        }
    } else {
        quote! {}
    };

    // Body-size cap: apply as a DefaultBodyLimit layer so an over-limit body
    // is rejected by the extractor before the handler runs.
    let body_limit_value = service_def.body_limit.unwrap_or(DEFAULT_BODY_LIMIT);

    // Startup assertion: a service with any WITH_PERMISSIONS method (or a
    // gated explorer) needs an auth provider, else every such call silently fails
    // authentication at runtime. Fail the build instead.
    let any_route_requires_auth = service_def
        .methods
        .iter()
        .any(|method| matches!(method.auth, AuthRequirement::WithPermissions(_)))
        || (explorer_enabled && service_def.docs_require_auth);
    let provider_check = if any_route_requires_auth {
        quote! {
            if self.auth_provider.is_none() {
                return Err(concat!(
                    "JSON-RPC service `",
                    #service_name_str,
                    "` has methods requiring authorization (WITH_PERMISSIONS) but no ",
                    "auth_provider was configured; call .auth_provider(...) before build()"
                )
                .to_string());
            }
        }
    } else {
        quote! {}
    };

    // Apply the explorer's auth policy where the service auth configuration
    // is available.
    let explorer_route_integration = if explorer_enabled {
        let service_name_lower = service_name_str.to_lowercase();
        let explorer_routes_fn_str = [&service_name_lower, "_explorer_routes"].concat();
        let explorer_routes_fn = syn::Ident::new(&explorer_routes_fn_str, service_name.span());
        if service_def.docs_require_auth {
            quote! {
                {
                    let __ras_docs_service = service.clone();
                    // `route_layer` (not `layer`) so the gate runs only for the
                    // explorer/openrpc routes that actually match — an unrelated
                    // path 404s without the middleware, and the RPC endpoint
                    // (a separate route) is never gated.
                    let __ras_explorer = #explorer_routes_fn(&base_url).route_layer(
                        axum::middleware::from_fn(move |__ras_req: axum::extract::Request, __ras_next: axum::middleware::Next| {
                            let __ras_docs_service = __ras_docs_service.clone();
                            async move {
                                use axum::response::IntoResponse;
                                let __ras_headers = __ras_req.headers().clone();
                                let __ras_empty: Vec<Vec<String>> = Vec::new();
                                match ras_jsonrpc_core::authorize_request(
                                    "GET",
                                    &__ras_headers,
                                    &__ras_docs_service.auth_transport,
                                    __ras_docs_service.auth_provider.as_deref(),
                                    &__ras_empty,
                                ).await {
                                    Ok(_) => __ras_next.run(__ras_req).await,
                                    Err(__ras_err) => {
                                        let __ras_status = match __ras_err {
                                            ras_jsonrpc_core::AuthorizeError::CsrfValidationFailed
                                            | ras_jsonrpc_core::AuthorizeError::InsufficientPermissions(_) =>
                                                axum::http::StatusCode::FORBIDDEN,
                                            ras_jsonrpc_core::AuthorizeError::NoAuthProvider =>
                                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                            _ => axum::http::StatusCode::UNAUTHORIZED,
                                        };
                                        ras_jsonrpc_core::tracing::warn!(status = __ras_status.as_u16(), "explorer request rejected");
                                        (
                                            __ras_status,
                                            [("Content-Type", "application/json")],
                                            serde_json::json!({ "error": "Authentication required" }).to_string(),
                                        ).into_response()
                                    }
                                }
                            }
                        })
                    );
                    router = router.merge(__ras_explorer);
                }
            }
        } else {
            quote! { router = router.merge(#explorer_routes_fn(&base_url)); }
        }
    } else {
        quote! {}
    };

    // Wire names of OPTIONAL_AUTH methods (canonical + legacy versions). The
    // request-level auth step rejects bad credentials globally; for these methods
    // we instead downgrade to anonymous so the route stays lenient/public.
    let optional_auth_wire_names: Vec<String> = service_def
        .methods
        .iter()
        .filter(|method| matches!(method.auth, AuthRequirement::OptionalAuth))
        .flat_map(|method| {
            std::iter::once(jsonrpc_method_wire_name(method)).chain(
                method
                    .versions
                    .iter()
                    .map(|version| version.wire_name.clone()),
            )
        })
        .collect();
    let optional_method_check = if optional_auth_wire_names.is_empty() {
        quote! { false }
    } else {
        quote! { matches!(request.method.as_str(), #(#optional_auth_wire_names)|*) }
    };

    let method_dispatch = service_def
        .methods
        .iter()
        .flat_map(generate_jsonrpc_method_dispatches);

    quote! {
            /// Build the axum router for the JSON-RPC service
            pub fn build(self) -> Result<axum::Router, String> {
                self.auth_transport
                    .validate()
                    .map_err(|err| err.to_string())?;

                #provider_check

                let base_url = self.base_url.clone();
                let service = std::sync::Arc::new(self);

                let rpc_handler = axum::routing::post({
                    // Clone into the handler so the outer `service` survives for
                    // the (optional) docs auth gate below.
                    let service = service.clone();
                    move |headers: axum::http::HeaderMap, body: String| {
                    let service = service.clone();
                    async move {
                        // Reject a non-`application/json` body with 415 before parsing.
                        #content_type_gate

                        let response = service.handle_request(headers, body).await;

                        // Determine HTTP status code based on JSON-RPC error code
                        // Map authentication/authorization errors to appropriate HTTP status codes
                        // while maintaining JSON-RPC protocol compatibility
                        let status_code = if let Some(ref error) = response.error {
                            match error.code {
                                ras_jsonrpc_types::error_codes::AUTHENTICATION_REQUIRED => axum::http::StatusCode::UNAUTHORIZED,
                                ras_jsonrpc_types::error_codes::INSUFFICIENT_PERMISSIONS => axum::http::StatusCode::FORBIDDEN,
                                ras_jsonrpc_types::error_codes::TOKEN_EXPIRED => axum::http::StatusCode::UNAUTHORIZED,
                                ras_jsonrpc_types::error_codes::CSRF_VALIDATION_FAILED => axum::http::StatusCode::FORBIDDEN,
                                _ => axum::http::StatusCode::OK, // Other JSON-RPC errors still return 200 OK
                            }
                        } else {
                            axum::http::StatusCode::OK
                        };

                        // Rejections otherwise bypass the usage/duration trackers
                        // (which run mid-dispatch); log auth/CSRF/permission
                        // rejections here so a bad-credential caller is observable.
                        if status_code != axum::http::StatusCode::OK {
                            if let Some(ref error) = response.error {
                                ras_jsonrpc_core::tracing::warn!(
                                    status = status_code.as_u16(),
                                    code = error.code,
                                    "JSON-RPC request rejected"
                                );
                            }
                        }

                        (
                            status_code,
                            [("Content-Type", "application/json")],
                            serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string())
                        )
                    }
                    }
                });

                let mut router = axum::Router::new();

                router = router.route(&base_url, rpc_handler);

                // Bound the request body size for the JSON-RPC endpoint.
                router = router.layer(axum::extract::DefaultBodyLimit::max(#body_limit_value));

                #explorer_route_integration

                Ok(router)
            }

            async fn handle_request(&self, headers: axum::http::HeaderMap, body: String) -> ras_jsonrpc_types::JsonRpcResponse {
                let request: ras_jsonrpc_types::JsonRpcRequest = match serde_json::from_str(&body) {
                    Ok(req) => req,
                    Err(__ras_json_err) => {
                        // Log the classification and location so the reason is
                        // recoverable server-side; never log the rejected value.
                        ras_jsonrpc_core::tracing::warn!(
                            category = ?__ras_json_err.classify(),
                            line = __ras_json_err.line(),
                            column = __ras_json_err.column(),
                            "rejected JSON-RPC request: malformed JSON body"
                        );
                        return ras_jsonrpc_types::JsonRpcResponse::error(ras_jsonrpc_types::JsonRpcError::parse_error(), None);
                    }
                };

                let request_id = request.id.clone();

                if request.jsonrpc != "2.0" {
                    return ras_jsonrpc_types::JsonRpcResponse::error(ras_jsonrpc_types::JsonRpcError::invalid_request(), request_id);
                }

                // Resolve the credential to Ok(Some/None) or Err(error response), then
                // apply a single downgrade decision: OPTIONAL_AUTH methods are public,
                // so any credential failure (failed CSRF, invalid/expired token)
                // downgrades to anonymous rather than rejecting the whole request.
                let __ras_method_is_optional = #optional_method_check;

                let auth_outcome: Result<
                    Option<ras_jsonrpc_core::AuthenticatedUser>,
                    ras_jsonrpc_types::JsonRpcResponse,
                > = if let Some(auth_provider) = &self.auth_provider {
                    match ras_jsonrpc_core::extract_auth_credential(&headers, &self.auth_transport) {
                        Ok(credential) => {
                            if ras_jsonrpc_core::validate_csrf_for_credential("POST", &headers, &credential, &self.auth_transport).is_err() {
                                Err(ras_jsonrpc_types::JsonRpcResponse::error(
                                    ras_jsonrpc_types::JsonRpcError::csrf_validation_failed(),
                                    request_id.clone(),
                                ))
                            } else {
                                match auth_provider.authenticate(credential.token().to_string()).await {
                                    Ok(user) => Ok(Some(user)),
                                    Err(ras_jsonrpc_core::AuthError::TokenExpired) => {
                                        Err(ras_jsonrpc_types::JsonRpcResponse::error(
                                            ras_jsonrpc_types::JsonRpcError::token_expired(),
                                            request_id.clone(),
                                        ))
                                    }
                                    Err(_) => Err(ras_jsonrpc_types::JsonRpcResponse::error(
                                        ras_jsonrpc_types::JsonRpcError::authentication_required(),
                                        request_id.clone(),
                                    )),
                                }
                            }
                        }
                        Err(ras_jsonrpc_core::AuthTransportError::MissingCredentials) => Ok(None),
                        Err(_) => Err(ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::authentication_required(),
                            request_id.clone(),
                        )),
                    }
                } else {
                    Ok(None)
                };

                let authenticated_user = match auth_outcome {
                    Ok(user) => user,
                    Err(error_response) => {
                        if __ras_method_is_optional {
                            None
                        } else {
                            return error_response;
                        }
                    }
                };

                if let Some(tracker) = &self.usage_tracker {
                    let user_ref = authenticated_user.as_ref();
                    let tracker_headers =
                        ras_jsonrpc_core::redact_sensitive_headers_for_auth_transport(&headers, &self.auth_transport);
                    tracker(&tracker_headers, user_ref, &request).await;
                }

                match request.method.as_str() {
                    #(#method_dispatch)*
                    _ => ras_jsonrpc_types::JsonRpcResponse::error(
                        ras_jsonrpc_types::JsonRpcError::method_not_found(&request.method),
                        request_id
                    )
                }
            }
    }
}
