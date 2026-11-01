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
                    .with_state((service, auth_provider, usage_tracker, method_duration_tracker, auth_transport))
            )
    }
}

/// The type of the boxed usage tracker the builder stores.
pub(super) fn usage_tracker_type() -> TokenStream {
    quote! {
        ::std::sync::Arc<
            dyn Fn(
                    &::axum::http::HeaderMap,
                    Option<&::ras_auth_core::AuthenticatedUser>,
                    &str,
                    &str,
                ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()> + Send>>
                + Send
                + Sync,
        >
    }
}

/// The type of the boxed method duration tracker the builder stores.
pub(super) fn duration_tracker_type() -> TokenStream {
    quote! {
        ::std::sync::Arc<
            dyn Fn(
                    &str,
                    &str,
                    Option<&::ras_auth_core::AuthenticatedUser>,
                    ::std::time::Duration,
                    bool,
                ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()> + Send>>
                + Send
                + Sync,
        >
    }
}

/// The router state every generated handler extracts.
pub(super) fn handler_state_type() -> TokenStream {
    let usage = usage_tracker_type();
    let duration = duration_tracker_type();
    quote! {
        (
            ::std::sync::Arc<S>,
            Option<::std::sync::Arc<A>>,
            Option<#usage>,
            Option<#duration>,
            ::ras_auth_core::AuthTransportConfig,
        )
    }
}

/// Wraps request decoding and the handler calls in the usage and duration trackers.
///
/// Authentication and the permission check run before this code and may return
/// early; such a rejection reaches neither tracker. `body` runs inside an async
/// block, so each early `return` there (a bad path, an invalid multipart part,
/// a handler error) still produces a response that the duration tracker records.
/// The request counts as a success unless the response status is 4xx or 5xx.
///
/// Expects `state`, `parts`, `method` and `user: Option<AuthenticatedUser>` in scope.
pub(super) fn tracked_handler(route: &str, body: TokenStream) -> TokenStream {
    quote! {
        if let Some(tracker) = &state.2 {
            let tracker_headers =
                ::ras_auth_core::redact_sensitive_headers_for_auth_transport(&parts.headers, &state.4);
            tracker(&tracker_headers, user.as_ref(), method, #route).await;
        }

        let __ras_start = ::std::time::Instant::now();
        let __ras_response: ::axum::response::Response = async {
            #body
        }
        .await;

        if let Some(tracker) = &state.3 {
            let __ras_status = __ras_response.status();
            let __ras_success = !(__ras_status.is_client_error() || __ras_status.is_server_error());
            tracker(method, #route, user.as_ref(), __ras_start.elapsed(), __ras_success).await;
        }

        __ras_response
    }
}
