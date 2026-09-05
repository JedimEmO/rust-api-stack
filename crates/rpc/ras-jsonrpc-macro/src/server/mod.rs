//! JSON-RPC service trait and builder.

use crate::ast::*;
use quote::quote;
mod dispatch;
mod http;

pub(crate) fn generate_server_code(service_def: &ServiceDefinition) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let service_trait_name = quote::format_ident!("{}Trait", service_name);
    let builder_name = quote::format_ident!("{}Builder", service_name);

    let trait_methods = service_def.methods.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        match &method.auth {
            AuthRequirement::Unauthorized => {
                quote! {
                    fn #method_name(&self, request: #request_type) -> impl std::future::Future<Output = Result<#response_type, Box<dyn std::error::Error + Send + Sync>>> + Send;
                }
            }
            AuthRequirement::OptionalAuth => {
                quote! {
                    fn #method_name(&self, caller: ras_jsonrpc_core::Caller, request: #request_type) -> impl std::future::Future<Output = Result<#response_type, Box<dyn std::error::Error + Send + Sync>>> + Send;
                }
            }
            AuthRequirement::WithPermissions(_) => {
                quote! {
                    fn #method_name(&self, user: &ras_jsonrpc_core::AuthenticatedUser, request: #request_type) -> impl std::future::Future<Output = Result<#response_type, Box<dyn std::error::Error + Send + Sync>>> + Send;
                }
            }
        }
    });

    let http_methods = http::generate_http_methods(service_def);

    quote! {
        /// Generated service trait
        #[allow(private_interfaces, private_bounds)]
        pub trait #service_trait_name: Send + Sync + 'static {
            #(#trait_methods)*
        }

        /// Generated builder for the JSON-RPC service
        pub struct #builder_name<T: #service_trait_name> {
            base_url: String,
            service: std::sync::Arc<T>,
            auth_provider: Option<Box<dyn ras_jsonrpc_core::AuthProvider>>,
            auth_transport: ras_jsonrpc_core::AuthTransportConfig,
            usage_tracker: Option<Box<dyn Fn(&axum::http::HeaderMap, Option<&ras_jsonrpc_core::AuthenticatedUser>, &ras_jsonrpc_types::JsonRpcRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
            method_duration_tracker: Option<Box<dyn Fn(&str, Option<&ras_jsonrpc_core::AuthenticatedUser>, std::time::Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
        }

        impl<T: #service_trait_name> #builder_name<T> {
            /// Create a new builder with the service implementation.
            ///
            /// The JSON-RPC route defaults to `/rpc`; use `base_url` to override it.
            pub fn new(service: T) -> Self {
                Self {
                    base_url: "/rpc".to_string(),
                    service: std::sync::Arc::new(service),
                    auth_provider: None,
                    auth_transport: ras_jsonrpc_core::AuthTransportConfig::default(),
                    usage_tracker: None,
                    method_duration_tracker: None,
                }
            }

            /// Override the JSON-RPC route path.
            pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
                self.base_url = base_url.into();
                self
            }

            /// Set the auth provider
            pub fn auth_provider<A: ras_jsonrpc_core::AuthProvider>(mut self, provider: A) -> Self {
                self.auth_provider = Some(Box::new(provider));
                self
            }

            /// Enable cookie authentication alongside bearer tokens.
            ///
            /// Installs a default double-submit CSRF config when none is set,
            /// because cookie credentials are CSRF-exploitable on unsafe methods.
            /// Override with `csrf_protection`.
            pub fn auth_cookie(mut self, cookie: ras_jsonrpc_core::AuthCookieConfig) -> Self {
                self.auth_transport.cookie = Some(cookie);
                if self.auth_transport.csrf.is_none() {
                    self.auth_transport.csrf = Some(ras_jsonrpc_core::CsrfConfig::default());
                }
                self
            }

            /// Replace the full auth transport configuration.
            pub fn auth_transport(mut self, transport: ras_jsonrpc_core::AuthTransportConfig) -> Self {
                self.auth_transport = transport;
                self
            }

            /// Require CSRF validation for cookie-authenticated JSON-RPC requests.
            pub fn csrf_protection(mut self, csrf: ras_jsonrpc_core::CsrfConfig) -> Self {
                self.auth_transport.csrf = Some(csrf);
                self
            }

            /// Set the usage tracker function
            /// This function will be called for each request with headers, authenticated user (if any), and the JSON-RPC request
            pub fn with_usage_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&axum::http::HeaderMap, Option<&ras_jsonrpc_core::AuthenticatedUser>, &ras_jsonrpc_types::JsonRpcRequest) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.usage_tracker = Some(Box::new(move |headers, user, request| {
                    Box::pin(tracker(headers, user, request))
                }));
                self
            }

            /// Set the method duration tracker function
            /// This function will be called after each method completes with the method name, authenticated user (if any), and the duration
            pub fn with_method_duration_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&str, Option<&ras_jsonrpc_core::AuthenticatedUser>, std::time::Duration) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.method_duration_tracker = Some(Box::new(move |method, user, duration| {
                    Box::pin(tracker(method, user, duration))
                }));
                self
            }

            #http_methods
        }
    }
}
