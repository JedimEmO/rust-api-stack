//! Method authorization, parameter decoding, invocation, and version adaptation.

use crate::ast::*;
use quote::quote;
use syn::{Ident, Type};

pub(super) fn jsonrpc_method_wire_name(method: &MethodDefinition) -> String {
    method
        .wire_name
        .clone()
        .unwrap_or_else(|| method.name.to_string())
}

fn jsonrpc_permission_groups_code(auth: &AuthRequirement) -> proc_macro2::TokenStream {
    let permission_groups = match auth {
        AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => Vec::new(),
        AuthRequirement::WithPermissions(groups) => groups.clone(),
    };

    if permission_groups.is_empty() {
        quote! { Vec::<Vec<String>>::new() }
    } else {
        let groups = permission_groups.iter().map(|group| {
            let perms = group.iter();
            quote! { vec![#(#perms.to_string()),*] }
        });
        quote! { vec![#(#groups),*] as Vec<Vec<String>> }
    }
}

fn jsonrpc_auth_check_code(
    auth: &AuthRequirement,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    match auth {
        AuthRequirement::Unauthorized => (quote! {}, quote! { None }),
        AuthRequirement::OptionalAuth => (
            quote! {
                // OPTIONAL_AUTH: surface the (optional) caller. The request-level
                // auth step already resolved `authenticated_user` best-effort; a
                // present-but-bad credential was downgraded to None for this method.
                // Cloned because `authenticated_user` is still needed for tracking below.
                let caller = ras_jsonrpc_core::Caller::from(authenticated_user.clone());
            },
            quote! { authenticated_user.as_ref() },
        ),
        AuthRequirement::WithPermissions(_) => {
            let permission_groups_code = jsonrpc_permission_groups_code(auth);
            (
                quote! {
                    let user = match &authenticated_user {
                        Some(u) => u,
                        None => return ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::authentication_required(),
                            request.id.clone()
                        ),
                    };

                    // OR-of-AND permission check (shared ras-auth-core implementation)
                    let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
                    let provider = self.auth_provider.as_ref().expect("auth provider required for WITH_PERMISSIONS methods");
                    if let Err(error) = ras_jsonrpc_core::check_permission_groups(provider.as_ref(), user, &required_permission_groups) {
                        // Only `required` is surfaced to the client; the caller's
                        // full grant set (`has`) stays server-side.
                        let required = match error {
                            ras_jsonrpc_core::AuthError::InsufficientPermissions { required, .. } => required,
                            _ => Vec::new(),
                        };
                        return ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::insufficient_permissions(required),
                            request.id.clone()
                        );
                    }
                },
                quote! { Some(user) },
            )
        }
    }
}

fn jsonrpc_parse_params_code(
    params_ident: &Ident,
    request_type: &Type,
) -> proc_macro2::TokenStream {
    quote! {
        let #params_ident: #request_type = match request.params {
            Some(params) => match serde_json::from_value(params) {
                Ok(p) => p,
                Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::invalid_params(e.to_string()),
                    request.id.clone()
                ),
            },
            None => match serde_json::from_value(serde_json::Value::Null) {
                Ok(p) => p,
                Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::invalid_params(e.to_string()),
                    request.id.clone()
                ),
            }
        };
    }
}

pub(super) fn generate_jsonrpc_method_dispatches(
    method: &MethodDefinition,
) -> Vec<proc_macro2::TokenStream> {
    let mut dispatches = vec![generate_jsonrpc_canonical_dispatch(method)];
    dispatches.extend(
        method
            .versions
            .iter()
            .map(|version| generate_jsonrpc_legacy_dispatch(method, version)),
    );
    dispatches
}

fn generate_jsonrpc_canonical_dispatch(method: &MethodDefinition) -> proc_macro2::TokenStream {
    let method_name = &method.name;
    let method_wire = jsonrpc_method_wire_name(method);
    let request_type = &method.request_type;
    let params_ident = quote::format_ident!("params");
    let parse_params = jsonrpc_parse_params_code(&params_ident, request_type);
    let (auth_check, tracker_user) = jsonrpc_auth_check_code(&method.auth);

    let handler_call = match &method.auth {
        AuthRequirement::Unauthorized => quote! { self.service.#method_name(#params_ident).await },
        AuthRequirement::OptionalAuth => {
            quote! { self.service.#method_name(caller, #params_ident).await }
        }
        AuthRequirement::WithPermissions(_) => {
            quote! { self.service.#method_name(user, #params_ident).await }
        }
    };

    quote! {
        #method_wire => {
            #auth_check
            #parse_params

            let start_time = std::time::Instant::now();
            let handler_result = #handler_call;
            let duration = start_time.elapsed();

            if let Some(duration_tracker) = &self.method_duration_tracker {
                duration_tracker(#method_wire, #tracker_user, duration).await;
            }

            match handler_result {
                Ok(result) => {
                    match serde_json::to_value(result) {
                        Ok(result_value) => ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id.clone()),
                        Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                            request.id.clone()
                        ),
                    }
                }
                Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                    request.id.clone()
                ),
            }
        }
    }
}

fn generate_jsonrpc_legacy_dispatch(
    method: &MethodDefinition,
    version: &MethodVersionDefinition,
) -> proc_macro2::TokenStream {
    let method_name = &method.name;
    let method_wire = &version.wire_name;
    let canonical_request_type = &method.request_type;
    let canonical_response_type = &method.response_type;
    let legacy_request_type = &version.request_type;
    let legacy_response_type = &version.response_type;
    let migration_type = &version.migration_type;
    let legacy_params_ident = quote::format_ident!("legacy_params");
    let params_ident = quote::format_ident!("params");
    let parse_params = jsonrpc_parse_params_code(&legacy_params_ident, legacy_request_type);
    let (auth_check, tracker_user) = jsonrpc_auth_check_code(&method.auth);

    let handler_call = match &method.auth {
        AuthRequirement::Unauthorized => quote! { self.service.#method_name(#params_ident).await },
        AuthRequirement::OptionalAuth => {
            quote! { self.service.#method_name(caller, #params_ident).await }
        }
        AuthRequirement::WithPermissions(_) => {
            quote! { self.service.#method_name(user, #params_ident).await }
        }
    };

    quote! {
        #method_wire => {
            #auth_check
            #parse_params

            let #params_ident: #canonical_request_type =
                match <#migration_type as ras_jsonrpc_core::VersionMigration<#legacy_request_type, #canonical_request_type>>::migrate(#legacy_params_ident) {
                    Ok(params) => params,
                    Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                        ras_jsonrpc_types::JsonRpcError::invalid_params(e.to_string()),
                        request.id.clone()
                    ),
                };

            let start_time = std::time::Instant::now();
            let handler_result = #handler_call;
            let duration = start_time.elapsed();

            if let Some(duration_tracker) = &self.method_duration_tracker {
                duration_tracker(#method_wire, #tracker_user, duration).await;
            }

            match handler_result {
                Ok(result) => {
                    let result: #legacy_response_type =
                        match <#migration_type as ras_jsonrpc_core::VersionMigration<#canonical_response_type, #legacy_response_type>>::migrate(result) {
                            Ok(result) => result,
                            Err(e) => return ras_jsonrpc_types::JsonRpcResponse::error(
                                ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                                request.id.clone()
                            ),
                        };

                    match serde_json::to_value(result) {
                        Ok(result_value) => ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id.clone()),
                        Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                            request.id.clone()
                        ),
                    }
                }
                Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                    ras_jsonrpc_types::JsonRpcError::internal_error(e.to_string()),
                    request.id.clone()
                ),
            }
        }
    }
}
