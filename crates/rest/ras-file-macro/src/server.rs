use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::LitStr;

use crate::parser::{AuthRequirement, Endpoint, FileServiceDefinition, Operation};

pub fn generate_server(definition: &FileServiceDefinition) -> TokenStream {
    let service_name = &definition.service_name;
    let base_path = &definition.base_path;

    let trait_name = format_ident!("{}Trait", service_name);
    let builder_name = format_ident!("{}Builder", service_name);
    let error_name = format_ident!("{}FileError", service_name);

    let trait_methods = generate_trait_methods(&definition.endpoints, &error_name);
    let handler_functions = generate_handlers(
        &definition.endpoints,
        &trait_name,
        &error_name,
        definition.body_limit,
    );
    let router_construction =
        generate_router_construction(&definition.endpoints, base_path, definition.body_limit);

    quote! {
        #[async_trait::async_trait]
        pub trait #trait_name: Send + Sync {
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
            S: #trait_name + Clone + Send + Sync + 'static,
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

        #[derive(Debug, ::thiserror::Error)]
        pub enum #error_name {
            #[error("File not found")]
            NotFound,
            #[error("Upload failed: {0}")]
            UploadFailed(String),
            #[error("Download failed: {0}")]
            DownloadFailed(String),
            #[error("Invalid file format")]
            InvalidFormat,
            #[error("File too large")]
            FileTooLarge,
            #[error("Internal error: {0}")]
            Internal(String),
        }

        impl ::axum::response::IntoResponse for #error_name {
            fn into_response(self) -> ::axum::response::Response {
                use ::axum::http::StatusCode;

                let (status, message) = match self {
                    #error_name::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
                    #error_name::InvalidFormat => (StatusCode::BAD_REQUEST, self.to_string()),
                    #error_name::FileTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
                    #error_name::UploadFailed(_) => (StatusCode::BAD_REQUEST, "Upload failed".to_string()),
                    #error_name::DownloadFailed(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Download failed".to_string()),
                    #error_name::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string()),
                };

                <(::axum::http::StatusCode, String) as ::axum::response::IntoResponse>::into_response((status, message))
            }
        }

        #handler_functions
    }
}

fn generate_trait_methods(endpoints: &[Endpoint], error_name: &Ident) -> TokenStream {
    let methods = endpoints.iter().map(|endpoint| {
        let method_name = &endpoint.name;
        let auth_param = match &endpoint.auth {
            AuthRequirement::Unauthorized => quote! {},
            AuthRequirement::WithPermissions(_) => {
                quote! { user: &::ras_auth_core::AuthenticatedUser, }
            }
        };

        let path_params = endpoint.path_params.iter().map(|param| {
            let name = &param.name;
            let ty = &param.ty;
            quote! { #name: #ty, }
        });

        match &endpoint.operation {
            Operation::Upload => {
                let response_type = endpoint
                    .response_type
                    .as_ref()
                    .map(|t| quote! { #t })
                    .unwrap_or_else(|| quote! { () });

                quote! {
                    async fn #method_name(
                        &self,
                        #auth_param
                        #(#path_params)*
                        multipart: ::axum::extract::Multipart
                    ) -> Result<#response_type, #error_name>;
                }
            }
            Operation::Download => {
                quote! {
                    async fn #method_name(
                        &self,
                        #auth_param
                        #(#path_params)*
                    ) -> Result<impl ::axum::response::IntoResponse, #error_name>;
                }
            }
        }
    });

    quote! { #(#methods)* }
}

fn generate_handlers(
    endpoints: &[Endpoint],
    trait_name: &Ident,
    error_name: &Ident,
    _body_limit: Option<u64>,
) -> TokenStream {
    endpoints.iter().map(|endpoint| {
        let handler_name = format_ident!("{}_handler", endpoint.name);
        let method_name = &endpoint.name;

        let auth_check = generate_auth_check(&endpoint.auth);
        let permission_check = generate_permission_check(&endpoint.auth);

        let path_extraction = if !endpoint.path_params.is_empty() {
            let param_names: Vec<_> = endpoint.path_params.iter().map(|p| &p.name).collect();
            let param_types: Vec<_> = endpoint.path_params.iter().map(|p| &p.ty).collect();
            quote! {
                let ::axum::extract::Path((#(#param_names,)*)) = match <::axum::extract::Path<(#(#param_types,)*)> as ::axum::extract::FromRequestParts<_>>::from_request_parts(&mut parts, &state).await {
                    Ok(path) => path,
                    Err(e) => return <(::axum::http::StatusCode, String) as ::axum::response::IntoResponse>::into_response(
                        (::axum::http::StatusCode::BAD_REQUEST, format!("Invalid path parameters: {}", e))
                    ),
                };
            }
        } else {
            quote! {}
        };

        let method_call = match &endpoint.operation {
            Operation::Upload => {
                let auth_arg = match &endpoint.auth {
                    AuthRequirement::Unauthorized => quote! {},
                    AuthRequirement::WithPermissions(_) => quote! { &user, },
                };
                let path_args = endpoint.path_params.iter().map(|p| {
                    let name = &p.name;
                    quote! { #name, }
                });

                quote! {
                    service.0.#method_name(#auth_arg #(#path_args)* multipart).await
                }
            }
            Operation::Download => {
                let auth_arg = match &endpoint.auth {
                    AuthRequirement::Unauthorized => quote! {},
                    AuthRequirement::WithPermissions(_) => quote! { &user, },
                };
                let path_args = endpoint.path_params.iter().map(|p| {
                    let name = &p.name;
                    quote! { #name, }
                });

                quote! {
                    service.0.#method_name(#auth_arg #(#path_args)*).await
                }
            }
        };

        let multipart_extraction = if let Operation::Upload = &endpoint.operation {
            // Always use the same extraction, body limit is applied at router level
            quote! {
                let multipart = match <::axum::extract::Multipart as ::axum::extract::FromRequest<_, _>>::from_request(req, &state).await {
                    Ok(mp) => mp,
                    Err(e) => return <(::axum::http::StatusCode, String) as ::axum::response::IntoResponse>::into_response((::axum::http::StatusCode::BAD_REQUEST, format!("Invalid multipart data: {}", e))),
                };
            }
        } else {
            quote! {}
        };

        match &endpoint.operation {
            Operation::Upload => quote! {
                async fn #handler_name<S, A>(
                    state: ::axum::extract::State<(
                        ::std::sync::Arc<S>,
                        Option<::std::sync::Arc<A>>,
                        Option<::std::sync::Arc<Box<dyn Fn(&::axum::http::HeaderMap, &str, &str) + Send + Sync>>>,
                        Option<::std::sync::Arc<Box<dyn Fn(&str, &str, std::time::Duration) + Send + Sync>>>,
                        ::ras_auth_core::AuthTransportConfig,
                    )>,
                    mut req: ::axum::http::Request<::axum::body::Body>,
                ) -> impl ::axum::response::IntoResponse
                where
                    S: #trait_name + Send + Sync + 'static,
                    A: ::ras_auth_core::AuthProvider + Send + Sync + 'static,
                {
                    let start = std::time::Instant::now();
                    let method = "POST";
                    let path = req.uri().path().to_string();

                    let (mut parts, body) = req.into_parts();

                    // Track usage
                    if let Some(tracker) = &state.2 {
                        let tracker_headers =
                            ::ras_auth_core::redact_sensitive_headers_for_auth_transport(&parts.headers, &state.4);
                        tracker(&tracker_headers, method, &path);
                    }

                    #auth_check
                    #permission_check

                    #path_extraction

                    // Reconstruct request for multipart extraction
                    let req = ::axum::http::Request::from_parts(parts, body);
                    #multipart_extraction

                    let service = &state.0;
                    let result = #method_call;

                    // Track duration
                    if let Some(tracker) = &state.3 {
                        tracker(method, &path, start.elapsed());
                    }

                    match result {
                        Ok(response) => <::axum::Json<_> as ::axum::response::IntoResponse>::into_response(::axum::Json(response)),
                        Err(e) => <#error_name as ::axum::response::IntoResponse>::into_response(e),
                    }
                }
            },
            Operation::Download => quote! {
                async fn #handler_name<S, A>(
                    state: ::axum::extract::State<(
                        ::std::sync::Arc<S>,
                        Option<::std::sync::Arc<A>>,
                        Option<::std::sync::Arc<Box<dyn Fn(&::axum::http::HeaderMap, &str, &str) + Send + Sync>>>,
                        Option<::std::sync::Arc<Box<dyn Fn(&str, &str, std::time::Duration) + Send + Sync>>>,
                        ::ras_auth_core::AuthTransportConfig,
                    )>,
                    req: ::axum::http::Request<::axum::body::Body>,
                ) -> impl ::axum::response::IntoResponse
                where
                    S: #trait_name + Send + Sync + 'static,
                    A: ::ras_auth_core::AuthProvider + Send + Sync + 'static,
                {
                    let start = std::time::Instant::now();
                    let method = "GET";
                    let path = req.uri().path().to_string();

                    let (mut parts, _body) = req.into_parts();

                    // Track usage
                    if let Some(tracker) = &state.2 {
                        let tracker_headers =
                            ::ras_auth_core::redact_sensitive_headers_for_auth_transport(&parts.headers, &state.4);
                        tracker(&tracker_headers, method, &path);
                    }

                    #auth_check
                    #permission_check

                    #path_extraction

                    let service = &state.0;
                    let result = #method_call;

                    // Track duration
                    if let Some(tracker) = &state.3 {
                        tracker(method, &path, start.elapsed());
                    }

                    match result {
                        Ok(response) => <_ as ::axum::response::IntoResponse>::into_response(response),
                        Err(e) => <#error_name as ::axum::response::IntoResponse>::into_response(e),
                    }
                }
            },
        }
    }).collect()
}

fn generate_auth_check(auth: &AuthRequirement) -> TokenStream {
    match auth {
        AuthRequirement::Unauthorized => quote! {
            let user = ::ras_auth_core::AuthenticatedUser {
                user_id: String::new(),
                permissions: ::std::collections::HashSet::new(),
                metadata: None,
            };
        },
        AuthRequirement::WithPermissions(_) => quote! {
            let auth_provider = match state.1.as_ref() {
                Some(provider) => provider,
                None => return <(::axum::http::StatusCode, &str) as ::axum::response::IntoResponse>::into_response(
                    (::axum::http::StatusCode::INTERNAL_SERVER_ERROR, "No auth provider configured")
                ),
            };

            let auth_credential = match ::ras_auth_core::extract_auth_credential(&parts.headers, &state.4) {
                Ok(credential) => credential,
                Err(_) => return <(::axum::http::StatusCode, &str) as ::axum::response::IntoResponse>::into_response(
                    (::axum::http::StatusCode::UNAUTHORIZED, "Missing or invalid authorization header")
                ),
            };

            if let Err(_) = ::ras_auth_core::validate_csrf_for_credential(method, &parts.headers, &auth_credential, &state.4) {
                return <(::axum::http::StatusCode, &str) as ::axum::response::IntoResponse>::into_response(
                    (::axum::http::StatusCode::FORBIDDEN, "CSRF validation failed")
                );
            }

            let user = match auth_provider.authenticate(auth_credential.token().to_string()).await {
                Ok(u) => u,
                Err(_) => return <(::axum::http::StatusCode, &str) as ::axum::response::IntoResponse>::into_response(
                    (::axum::http::StatusCode::UNAUTHORIZED, "Invalid authentication")
                ),
            };
        },
    }
}

fn generate_permission_check(auth: &AuthRequirement) -> TokenStream {
    match auth {
        AuthRequirement::Unauthorized => quote! {},
        AuthRequirement::WithPermissions(permission_groups) => {
            let group_checks = permission_groups.iter().map(|group| {
                let permission_checks = group.iter().map(|perm| {
                    quote! { user.permissions.contains(#perm) }
                });
                quote! { #(#permission_checks)&&* }
            });

            quote! {
                let has_permission = #(#group_checks)||*;
                if !has_permission {
                    return <(::axum::http::StatusCode, &str) as ::axum::response::IntoResponse>::into_response((::axum::http::StatusCode::FORBIDDEN, "Insufficient permissions"));
                }
            }
        }
    }
}

fn generate_router_construction(
    endpoints: &[Endpoint],
    base_path: &LitStr,
    body_limit: Option<u64>,
) -> TokenStream {
    let routes = endpoints.iter().map(|endpoint| {
        let handler_name = format_ident!("{}_handler", endpoint.name);
        let path = endpoint
            .path
            .as_ref()
            .map(|p| {
                let path_str = p.value();
                if path_str.starts_with('/') {
                    path_str
                } else {
                    format!("/{}", path_str)
                }
            })
            .unwrap_or_else(|| format!("/{}", endpoint.name));

        let http_method = match &endpoint.operation {
            Operation::Upload => quote! { post },
            Operation::Download => quote! { get },
        };

        quote! {
            .route(#path, #http_method(#handler_name::<S, A>))
        }
    });

    let router_with_limit = if let Some(limit) = body_limit {
        let limit_usize = limit as usize;
        quote! {
            ::axum::Router::new()
                #(#routes)*
                .layer(::axum::extract::DefaultBodyLimit::max(#limit_usize))
                .with_state((service, auth_provider, usage_tracker, duration_tracker, auth_transport))
        }
    } else {
        quote! {
            ::axum::Router::new()
                #(#routes)*
                .with_state((service, auth_provider, usage_tracker, duration_tracker, auth_transport))
        }
    };

    quote! {
        ::axum::Router::new()
            .nest(
                #base_path,
                #router_with_limit
            )
    }
}
