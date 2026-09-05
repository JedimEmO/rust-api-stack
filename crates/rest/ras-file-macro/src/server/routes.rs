use crate::parser::{Endpoint, MaxBytes, Operation, PathParam};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) fn generate_path_extraction(
    path_params: &[PathParam],
    path_struct: &Ident,
) -> TokenStream {
    if path_params.is_empty() {
        return quote! { let path_value = #path_struct {}; };
    }

    let fields = path_params.iter().enumerate().map(|(idx, param)| {
        let name = &param.name;
        if path_params.len() == 1 {
            quote! { #name: path_params }
        } else {
            let idx = syn::Index::from(idx);
            quote! { #name: path_params.#idx }
        }
    });

    let extraction = if path_params.len() == 1 {
        let ty = &path_params[0].ty;
        quote! {
            let ::axum::extract::Path(path_params) =
                match <::axum::extract::Path<#ty> as ::axum::extract::FromRequestParts<_>>::from_request_parts(&mut parts, &state).await {
                    Ok(path) => path,
                    Err(error) => {
                        // The axum rejection echoes the offending path value;
                        // log it server-side and send a fixed message.
                        ::ras_file_core::tracing::warn!(
                            status = error.status().as_u16(),
                            detail = %::ras_file_core::sanitize_log_detail(&error.body_text()),
                            "rejected request: invalid path parameters"
                        );
                        return __ras_file_error_response(::ras_file_core::FileError::bad_request("invalid path parameters"));
                    }
                };
        }
    } else {
        let tys = path_params.iter().map(|param| &param.ty);
        quote! {
            let ::axum::extract::Path(path_params) =
                match <::axum::extract::Path<(#(#tys),*)> as ::axum::extract::FromRequestParts<_>>::from_request_parts(&mut parts, &state).await {
                    Ok(path) => path,
                    Err(error) => {
                        // The axum rejection echoes the offending path value;
                        // log it server-side and send a fixed message.
                        ::ras_file_core::tracing::warn!(
                            status = error.status().as_u16(),
                            detail = %::ras_file_core::sanitize_log_detail(&error.body_text()),
                            "rejected request: invalid path parameters"
                        );
                        return __ras_file_error_response(::ras_file_core::FileError::bad_request("invalid path parameters"));
                    }
                };
        }
    };

    quote! {
        #extraction
        let path_value = #path_struct {
            #(#fields),*
        };
    }
}

pub(super) fn generate_router_construction(
    endpoints: &[Endpoint],
    base_path: &syn::LitStr,
) -> TokenStream {
    let routes = endpoints.iter().map(|endpoint| {
        let handler_name = format_ident!("{}_handler", endpoint.name);
        let path = endpoint.path.value();

        match &endpoint.operation {
            Operation::Upload { config, .. } => {
                let limit_layer = match &config.max_total_bytes {
                    MaxBytes::Limited(limit) => {
                        let limit = *limit as usize;
                        quote! { .layer(::axum::extract::DefaultBodyLimit::max(#limit)) }
                    }
                    MaxBytes::Unlimited => {
                        quote! { .layer(::axum::extract::DefaultBodyLimit::disable()) }
                    }
                };
                quote! {
                    .route(#path, post(#handler_name::<S, A>)#limit_layer)
                }
            }
            Operation::Download { .. } => quote! {
                .route(#path, get(#handler_name::<S, A>))
            },
        }
    });

    quote! {
        ::axum::Router::new()
            .nest(
                #base_path,
                ::axum::Router::new()
                    #(#routes)*
                    .with_state((service, auth_provider, usage_tracker, duration_tracker, auth_transport))
            )
    }
}
