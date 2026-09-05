//! Server trait, builder, and route assembly.

use crate::{ast::*, static_hosting};
use quote::quote;
mod auth;
mod handlers;
mod request;
mod routes;
mod versioned;
use request::{generate_query_struct, generate_rest_request_part_structs};
use routes::{generate_canonical_route_registration, generate_legacy_route_registration};

pub(crate) fn generate_server_code(
    service_def: &ServiceDefinition,
    schema_checks: proc_macro2::TokenStream,
    openapi_code: proc_macro2::TokenStream,
    static_hosting_code: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let service_trait_name = quote::format_ident!("{}Trait", service_name);
    let builder_name = quote::format_ident!("{}Builder", service_name);
    let base_path = &service_def.base_path;
    let trait_methods = service_def.endpoints.iter().map(|endpoint| {
        let handler_name = &endpoint.handler_name;
        let response_type = &endpoint.response_type;

        let mut params = Vec::new();
        match &endpoint.auth {
            AuthRequirement::Unauthorized => {}
            AuthRequirement::OptionalAuth => {
                params.push(quote! { caller: ras_auth_core::Caller });
            }
            AuthRequirement::WithPermissions(_) => {
                params.push(quote! { user: &ras_auth_core::AuthenticatedUser });
            }
        }

        // Opt-in request headers (immediately after the caller/user)
        if endpoint.with_headers {
            params.push(quote! { headers: axum::http::HeaderMap });
        }

        for path_param in &endpoint.path_params {
            let param_name = &path_param.name;
            let param_type = &path_param.param_type;
            params.push(quote! { #param_name: #param_type });
        }

        for query_param in &endpoint.query_params {
            let param_name = &query_param.name;
            let param_type = &query_param.param_type;
            params.push(quote! { #param_name: #param_type });
        }

        if let Some(request_type) = &endpoint.request_type {
            params.push(quote! { request: #request_type });
        }

        quote! {
            async fn #handler_name(&self, #(#params),*) -> ras_rest_core::RestResult<#response_type>;
        }
    });

    let request_part_structs = generate_rest_request_part_structs(service_def);

    let mut query_structs: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut route_registrations: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut route_idx = 0usize;

    for endpoint in &service_def.endpoints {
        let query_struct_name = quote::format_ident!("QueryParams{}", route_idx);
        query_structs.push(generate_query_struct(
            &query_struct_name,
            &endpoint.query_params,
        ));
        route_registrations.push(generate_canonical_route_registration(
            endpoint,
            &query_struct_name,
            service_def.require_json_content_type,
        ));
        route_idx += 1;

        for version in &endpoint.versions {
            let query_struct_name = quote::format_ident!("QueryParams{}", route_idx);
            query_structs.push(generate_query_struct(
                &query_struct_name,
                &version.query_params,
            ));
            route_registrations.push(generate_legacy_route_registration(
                &service_def.service_name,
                endpoint,
                version,
                &query_struct_name,
                service_def.require_json_content_type,
            ));
            route_idx += 1;
        }
    }

    let static_routes = if service_def.static_hosting.serve_docs {
        static_hosting::generate_static_routes(service_def, &service_def.static_hosting)
    } else {
        quote! {}
    };

    let body_limit = service_def.body_limit.unwrap_or(DEFAULT_BODY_LIMIT);

    // Startup assertion: a service with any WITH_PERMISSIONS route needs an auth
    // provider, otherwise every such route returns a runtime 500 (NoAuthProvider)
    // on first request. Catch the misconfiguration at build() instead.
    let any_route_requires_auth = service_def
        .endpoints
        .iter()
        .any(|endpoint| matches!(endpoint.auth, AuthRequirement::WithPermissions(_)))
        || (service_def.static_hosting.serve_docs && service_def.docs_require_auth);
    let service_name_str = service_name.to_string();
    let provider_assertion = if any_route_requires_auth {
        quote! {
            if self.auth_provider.is_none() {
                panic!(concat!(
                    "REST service `",
                    #service_name_str,
                    "` has endpoints requiring authorization (WITH_PERMISSIONS) but no ",
                    "auth_provider was configured; call .auth_provider(...) before build()"
                ));
            }
        }
    } else {
        quote! {}
    };

    quote! {
        /// Maximum accepted JSON body size in bytes
        #[allow(dead_code)]
        const __RAS_BODY_LIMIT: usize = #body_limit;

        /// Map a shared authorization failure to this service's JSON error shape
        #[allow(dead_code)]
        fn __ras_authorize_error_response(error: ras_auth_core::AuthorizeError) -> axum::response::Response {
            use axum::response::IntoResponse;
            let (status, message) = match &error {
                ras_auth_core::AuthorizeError::MissingCredential => (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Missing or invalid Authorization header",
                ),
                ras_auth_core::AuthorizeError::CsrfValidationFailed => (
                    axum::http::StatusCode::FORBIDDEN,
                    "CSRF validation failed",
                ),
                ras_auth_core::AuthorizeError::AuthenticationFailed(_) => (
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Authentication failed",
                ),
                ras_auth_core::AuthorizeError::NoAuthProvider => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "No auth provider configured",
                ),
                ras_auth_core::AuthorizeError::InsufficientPermissions(_) => (
                    axum::http::StatusCode::FORBIDDEN,
                    "Insufficient permissions",
                ),
            };
            // Rejections are otherwise invisible to the usage/duration trackers,
            // which only run on the post-auth happy path; log them here so a
            // client hammering an endpoint with a bad credential is observable.
            // The `error` detail is logged server-side only; the client gets the
            // generic `message`.
            ras_rest_core::tracing::warn!(
                status = status.as_u16(),
                error = ?error,
                "request rejected during authorization"
            );
            (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
        }

        /// Build a success response, omitting the JSON body for status codes that
        /// must not carry one (`204 No Content`, `205 Reset Content`,
        /// `304 Not Modified`). Without this, `RestResponse::no_content()` would
        /// emit a `204` with a serialized `null` body and
        /// `Content-Type: application/json`, which violates RFC 9110 and is
        /// rejected by some proxies and clients.
        #[allow(dead_code)]
        fn __ras_success_response<T: serde::Serialize>(
            status: axum::http::StatusCode,
            body: T,
        ) -> axum::response::Response {
            use axum::response::IntoResponse;
            if status == axum::http::StatusCode::NO_CONTENT
                || status == axum::http::StatusCode::RESET_CONTENT
                || status == axum::http::StatusCode::NOT_MODIFIED
            {
                status.into_response()
            } else {
                (status, axum::Json(body)).into_response()
            }
        }

        /// Generated service trait
        #[async_trait::async_trait]
        #[allow(private_interfaces, private_bounds)]
        pub trait #service_trait_name: Send + Sync + 'static {
            #(#trait_methods)*
        }

        /// Generated builder for the REST service
        pub struct #builder_name<T: #service_trait_name> {
            service: std::sync::Arc<T>,
            auth_provider: Option<std::sync::Arc<dyn ras_auth_core::AuthProvider>>,
            auth_transport: ras_auth_core::AuthTransportConfig,
            with_usage_tracker: Option<std::sync::Arc<dyn Fn(&axum::http::HeaderMap, Option<&ras_auth_core::AuthenticatedUser>, &str, &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
            with_method_duration_tracker: Option<std::sync::Arc<dyn Fn(&str, &str, Option<&ras_auth_core::AuthenticatedUser>, std::time::Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>>,
        }

        const _: () = {
            #schema_checks
        };

        #openapi_code

        #static_hosting_code

        use self::query_params::*;

        mod query_params {
            #[allow(unused_imports)]
            use super::*;

            #(#query_structs)*
        }

        #request_part_structs

        impl<T: #service_trait_name> #builder_name<T> {
            /// Create a new builder with the service implementation
            pub fn new(service: T) -> Self {
                Self {
                    service: std::sync::Arc::new(service),
                    auth_provider: None,
                    auth_transport: ras_auth_core::AuthTransportConfig::default(),
                    with_usage_tracker: None,
                    with_method_duration_tracker: None,
                }
            }

            /// Set the auth provider
            pub fn auth_provider<A: ras_auth_core::AuthProvider>(mut self, provider: A) -> Self {
                self.auth_provider = Some(std::sync::Arc::new(provider));
                self
            }

            /// Enable cookie authentication alongside bearer tokens.
            ///
            /// Installs a default double-submit CSRF config when none is set,
            /// because cookie credentials are CSRF-exploitable on unsafe methods.
            /// Override with `csrf_protection`.
            pub fn auth_cookie(mut self, cookie: ras_auth_core::AuthCookieConfig) -> Self {
                self.auth_transport.cookie = Some(cookie);
                if self.auth_transport.csrf.is_none() {
                    self.auth_transport.csrf = Some(ras_auth_core::CsrfConfig::default());
                }
                self
            }

            /// Replace the full auth transport configuration.
            pub fn auth_transport(mut self, transport: ras_auth_core::AuthTransportConfig) -> Self {
                self.auth_transport = transport;
                self
            }

            /// Require CSRF validation for cookie-authenticated unsafe requests.
            pub fn csrf_protection(mut self, csrf: ras_auth_core::CsrfConfig) -> Self {
                self.auth_transport.csrf = Some(csrf);
                self
            }

            /// Set the usage tracker - called before each request
            /// The tracker receives the headers, authenticated user (if any), HTTP method, and path
            pub fn with_usage_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&axum::http::HeaderMap, Option<&ras_auth_core::AuthenticatedUser>, &str, &str) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.with_usage_tracker = Some(std::sync::Arc::new(move |headers, user, method, path| {
                    Box::pin(tracker(headers, user, method, path))
                }));
                self
            }

            /// Set the method duration tracker - called after each request completes
            /// The tracker receives the HTTP method, path, authenticated user (if any), and execution duration
            pub fn with_method_duration_tracker<F, Fut>(mut self, tracker: F) -> Self
            where
                F: Fn(&str, &str, Option<&ras_auth_core::AuthenticatedUser>, std::time::Duration) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = ()> + Send + 'static,
            {
                self.with_method_duration_tracker = Some(std::sync::Arc::new(move |method, path, user, duration| {
                    Box::pin(tracker(method, path, user, duration))
                }));
                self
            }

            /// Build the axum router for the REST service
            pub fn build(self) -> axum::Router {
                self.auth_transport
                    .validate()
                    .expect("invalid auth transport configuration");

                #provider_assertion

                let mut router = axum::Router::new();

                #(#route_registrations)*

                #static_routes

                if #base_path.is_empty() || #base_path == "/" {
                    router
                } else {
                    axum::Router::new().nest(#base_path, router)
                }
            }
        }

    }
}
