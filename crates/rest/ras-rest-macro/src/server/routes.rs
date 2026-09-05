//! Canonical and versioned route registration.

use super::auth::rest_permission_groups_code;
use super::handlers::generate_handler_body;
use super::request::{AxumHandlerParts, generate_axum_handler};
use super::versioned::generate_legacy_handler_body;
use crate::ast::*;
use quote::quote;
use syn::Ident;

pub(super) fn generate_canonical_route_registration(
    endpoint: &EndpointDefinition,
    query_struct_name: &Ident,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let method_routing = endpoint.method.as_axum_method();
    let path = &endpoint.path;
    let handler_name = &endpoint.handler_name;
    let method_str = endpoint.method.as_str();
    let AxumHandlerParts {
        extractors: axum_handler,
        prelude: extractor_prelude,
    } = generate_axum_handler(
        &endpoint.path_params,
        &endpoint.query_params,
        endpoint.request_type.as_ref(),
        query_struct_name,
        method_str,
        path,
    );
    let handler_body =
        generate_handler_body(endpoint, handler_name, method_str, path, require_json);
    let permission_groups_code = rest_permission_groups_code(&endpoint.auth);

    quote! {
        {
            let service = self.service.clone();
            let auth_provider = self.auth_provider.clone();
            let auth_transport = self.auth_transport.clone();
            let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
            let with_usage_tracker = self.with_usage_tracker.clone();
            let with_method_duration_tracker = self.with_method_duration_tracker.clone();

            router = router.route(#path, #method_routing({
                move |#axum_handler| {
                    let service = service.clone();
                    let auth_provider = auth_provider.clone();
                    let auth_transport = auth_transport.clone();
                    let required_permission_groups: Vec<Vec<String>> = required_permission_groups.clone();
                    let with_usage_tracker = with_usage_tracker.clone();
                    let with_method_duration_tracker = with_method_duration_tracker.clone();

                    async move {
                        #extractor_prelude
                        #handler_body
                    }
                }
            }));
        }
    }
}

pub(super) fn generate_legacy_route_registration(
    service_name: &Ident,
    endpoint: &EndpointDefinition,
    version: &EndpointVersionDefinition,
    query_struct_name: &Ident,
    require_json: bool,
) -> proc_macro2::TokenStream {
    let method_routing = endpoint.method.as_axum_method();
    let path = &version.path;
    let AxumHandlerParts {
        extractors: axum_handler,
        prelude: extractor_prelude,
    } = generate_axum_handler(
        &version.path_params,
        &version.query_params,
        version.request_type.as_ref(),
        query_struct_name,
        endpoint.method.as_str(),
        path,
    );
    let handler_body = generate_legacy_handler_body(service_name, endpoint, version, require_json);
    let permission_groups_code = rest_permission_groups_code(&endpoint.auth);

    quote! {
        {
            let service = self.service.clone();
            let auth_provider = self.auth_provider.clone();
            let auth_transport = self.auth_transport.clone();
            let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
            let with_usage_tracker = self.with_usage_tracker.clone();
            let with_method_duration_tracker = self.with_method_duration_tracker.clone();

            router = router.route(#path, #method_routing({
                move |#axum_handler| {
                    let service = service.clone();
                    let auth_provider = auth_provider.clone();
                    let auth_transport = auth_transport.clone();
                    let required_permission_groups: Vec<Vec<String>> = required_permission_groups.clone();
                    let with_usage_tracker = with_usage_tracker.clone();
                    let with_method_duration_tracker = with_method_duration_tracker.clone();

                    async move {
                        #extractor_prelude
                        #handler_body
                    }
                }
            }));
        }
    }
}

impl HttpMethod {
    fn as_axum_method(&self) -> proc_macro2::TokenStream {
        match self {
            HttpMethod::Get => quote! { axum::routing::get },
            HttpMethod::Post => quote! { axum::routing::post },
            HttpMethod::Put => quote! { axum::routing::put },
            HttpMethod::Delete => quote! { axum::routing::delete },
            HttpMethod::Patch => quote! { axum::routing::patch },
        }
    }
}
