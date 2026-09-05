mod auth;
mod download;
mod routes;
mod types;
mod upload;

use crate::parser::{FileServiceDefinition, Operation};
use download::generate_download_handler;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use routes::generate_router_construction;
use types::{generate_support_types, generate_trait_methods};
use upload::generate_upload_handler;

pub fn generate_server(definition: &FileServiceDefinition) -> TokenStream {
    let service_name = &definition.service_name;
    let base_path = &definition.base_path;

    let trait_name = format_ident!("{}Trait", service_name);
    let builder_name = format_ident!("{}Builder", service_name);
    let error_name = format_ident!("{}FileError", service_name);

    let support_types = generate_support_types(definition);
    let trait_methods = generate_trait_methods(definition, &trait_name);
    let handler_functions = generate_handlers(definition, &trait_name);
    let router_construction = generate_router_construction(&definition.endpoints, base_path);

    quote! {
        pub type #error_name = ::ras_file_core::FileError;

        #support_types

        #[async_trait::async_trait]
        pub trait #trait_name: Send + Sync + 'static {
            #trait_methods
        }

        pub struct #builder_name<S, A> {
            service: S,
            auth_provider: Option<A>,
            auth_transport: ::ras_auth_core::AuthTransportConfig,
            usage_tracker: Option<Box<dyn Fn(&::axum::http::HeaderMap, &str, &str) + Send + Sync>>,
            duration_tracker: Option<Box<dyn Fn(&str, &str, std::time::Duration) + Send + Sync>>,
        }

        impl<S, A> #builder_name<S, A>
        where
            S: #trait_name + Send + Sync + 'static,
            A: ::ras_auth_core::AuthProvider + Clone + Send + Sync + 'static,
        {
            pub fn new(service: S) -> Self {
                Self {
                    service,
                    auth_provider: None,
                    auth_transport: ::ras_auth_core::AuthTransportConfig::default(),
                    usage_tracker: None,
                    duration_tracker: None,
                }
            }

            pub fn auth_provider(mut self, provider: A) -> Self {
                self.auth_provider = Some(provider);
                self
            }

            pub fn auth_cookie(mut self, cookie: ::ras_auth_core::AuthCookieConfig) -> Self {
                self.auth_transport.cookie = Some(cookie);
                if self.auth_transport.csrf.is_none() {
                    self.auth_transport.csrf = Some(::ras_auth_core::CsrfConfig::default());
                }
                self
            }

            pub fn auth_transport(mut self, transport: ::ras_auth_core::AuthTransportConfig) -> Self {
                self.auth_transport = transport;
                self
            }

            pub fn csrf_protection(mut self, csrf: ::ras_auth_core::CsrfConfig) -> Self {
                self.auth_transport.csrf = Some(csrf);
                self
            }

            pub fn with_usage_tracker<F>(mut self, tracker: F) -> Self
            where
                F: Fn(&::axum::http::HeaderMap, &str, &str) + Send + Sync + 'static,
            {
                self.usage_tracker = Some(Box::new(tracker));
                self
            }

            pub fn with_duration_tracker<F>(mut self, tracker: F) -> Self
            where
                F: Fn(&str, &str, std::time::Duration) + Send + Sync + 'static,
            {
                self.duration_tracker = Some(Box::new(tracker));
                self
            }

            pub fn build(self) -> ::axum::Router {
                use ::axum::routing::{get, post};

                self.auth_transport
                    .validate()
                    .expect("invalid auth transport configuration");

                let service = ::std::sync::Arc::new(self.service);
                let auth_provider = self.auth_provider.map(::std::sync::Arc::new);
                let auth_transport = self.auth_transport;
                let usage_tracker = self.usage_tracker.map(::std::sync::Arc::new);
                let duration_tracker = self.duration_tracker.map(::std::sync::Arc::new);

                #router_construction
            }
        }

        fn __ras_file_error_response(error: ::ras_file_core::FileError) -> ::axum::response::Response {
            use ::axum::response::IntoResponse;
            let status = error.status();
            let message = error.client_message();
            (
                status,
                ::axum::Json(::serde_json::json!({ "error": message })),
            ).into_response()
        }

        /// Map a multipart parse error to a `FileError`. The axum detail
        /// (which can echo field names and parser state) is logged at `warn`
        /// server-side; the client receives a fixed generic message.
        fn __ras_file_multipart_error(error: ::axum::extract::multipart::MultipartError) -> ::ras_file_core::FileError {
            if error.status() == ::axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                ::ras_file_core::FileError::PayloadTooLarge
            } else {
                ::ras_file_core::tracing::warn!(
                    status = error.status().as_u16(),
                    detail = %::ras_file_core::sanitize_log_detail(&error.body_text()),
                    "rejected request: invalid multipart body"
                );
                ::ras_file_core::FileError::bad_request("invalid multipart body")
            }
        }

        fn __ras_file_download_response(response: ::ras_file_core::DownloadResponse) -> ::axum::response::Response {
            use ::axum::response::IntoResponse;
            let mut builder = ::axum::response::Response::builder().status(response.status);
            let headers = builder.headers_mut().expect("response builder is valid before body");
            for (name, value) in response.headers.iter() {
                headers.insert(name.clone(), value.clone());
            }

            let body = match response.body {
                ::ras_file_core::DownloadBody::Empty => ::axum::body::Body::empty(),
                ::ras_file_core::DownloadBody::Bytes(bytes) => ::axum::body::Body::from(bytes),
                ::ras_file_core::DownloadBody::Stream(stream) => ::axum::body::Body::from_stream(stream),
            };

            builder
                .body(body)
                .unwrap_or_else(|_| {
                    (
                        ::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "failed to build file response",
                    ).into_response()
                })
        }

        async fn __ras_read_field_bytes(
            mut field: ::axum::extract::multipart::Field<'_>,
            max_bytes: u64,
            remaining_total: Option<u64>,
        ) -> ::ras_file_core::FileResult<::ras_file_core::bytes::Bytes> {
            let mut bytes = Vec::new();

            while let Some(chunk) = field.chunk().await.map_err(__ras_file_multipart_error)? {
                let next_len = bytes
                    .len()
                    .checked_add(chunk.len())
                    .ok_or(::ras_file_core::FileError::PayloadTooLarge)?;

                if next_len as u64 > max_bytes {
                    return Err(::ras_file_core::FileError::PayloadTooLarge);
                }

                if let Some(remaining_total) = remaining_total {
                    if next_len as u64 > remaining_total {
                        return Err(::ras_file_core::FileError::PayloadTooLarge);
                    }
                }

                bytes.extend_from_slice(&chunk);
            }

            Ok(::ras_file_core::bytes::Bytes::from(bytes))
        }

        #handler_functions
    }
}

fn generate_handlers(definition: &FileServiceDefinition, trait_name: &Ident) -> TokenStream {
    definition
        .endpoints
        .iter()
        .map(|endpoint| match &endpoint.operation {
            Operation::Upload { config, .. } => {
                generate_upload_handler(definition, endpoint, config, trait_name)
            }
            Operation::Download { .. } => {
                generate_download_handler(definition, endpoint, trait_name)
            }
        })
        .collect()
}
