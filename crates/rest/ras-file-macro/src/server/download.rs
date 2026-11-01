use super::auth::{generate_auth_check, generate_permission_check};
use super::routes::{generate_path_extraction, handler_state_type, tracked_handler};
use super::types::path_struct_name;
use crate::parser::{Endpoint, FileServiceDefinition};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) fn generate_download_handler(
    definition: &FileServiceDefinition,
    endpoint: &Endpoint,
    trait_name: &Ident,
) -> TokenStream {
    let handler_fn = format_ident!("{}_handler", endpoint.name);
    let handler_name = &endpoint.name;
    let path = endpoint.path.value();
    let path_struct = path_struct_name(&definition.service_name, endpoint);
    let auth = generate_auth_check(&endpoint.auth);
    let permission_check = generate_permission_check(&endpoint.auth);
    let path_extraction = generate_path_extraction(&endpoint.path_params, &path_struct);
    let state_type = handler_state_type();
    let tracked = tracked_handler(
        &path,
        quote! {
            #path_extraction

            let ctx = ::ras_file_core::FileRequestContext::new(
                method,
                &request_path,
                #path,
                &parts.headers,
                user.as_ref(),
            );

            let service = &state.0.0;
            match service.#handler_name(&ctx, path_value).await {
                Ok(response) => __ras_file_download_response(response),
                Err(error) => __ras_file_error_response(error),
            }
        },
    );

    quote! {
        async fn #handler_fn<S, A>(
            state: ::axum::extract::State<#state_type>,
            req: ::axum::http::Request<::axum::body::Body>,
        ) -> ::axum::response::Response
        where
            S: #trait_name + Send + Sync + 'static,
            A: ::ras_auth_core::AuthProvider + Send + Sync + 'static,
        {
            let method = "GET";
            let request_path = req.uri().path().to_string();
            let (mut parts, _body) = req.into_parts();

            #auth
            #permission_check

            #tracked
        }
    }
}
