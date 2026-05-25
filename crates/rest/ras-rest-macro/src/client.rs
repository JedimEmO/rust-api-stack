use crate::{EndpointDefinition, HttpMethod, ServiceDefinition};
use quote::quote;
use syn::{GenericArgument, PathArguments, Type};

/// Returns the inner type for syntactic wrappers like `Option<T>` or `Vec<T>`.
/// Matches bare and fully-qualified forms by checking the final path segment.
fn generic_inner_type<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let last = type_path.path.segments.last()?;
    if last.ident != wrapper {
        return None;
    }

    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };

    args.args.iter().find_map(|arg| {
        if let GenericArgument::Type(inner) = arg {
            Some(inner)
        } else {
            None
        }
    })
}

fn option_inner_type(ty: &Type) -> Option<&Type> {
    generic_inner_type(ty, "Option")
}

fn vec_inner_type(ty: &Type) -> Option<&Type> {
    generic_inner_type(ty, "Vec")
}

fn option_vec_inner_type(ty: &Type) -> Option<&Type> {
    option_inner_type(ty).and_then(vec_inner_type)
}

/// Generate client code for REST service
pub fn generate_client_code(service_def: &ServiceDefinition) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let client_name = quote::format_ident!("{}Client", service_name);
    let client_builder_name = quote::format_ident!("{}ClientBuilder", service_name);
    let base_path = &service_def.base_path;

    // Generate client methods
    let client_methods = service_def
        .endpoints
        .iter()
        .flat_map(generate_client_methods_for_endpoint);

    let client_methods_with_timeout = service_def
        .endpoints
        .iter()
        .flat_map(generate_client_methods_with_timeout_for_endpoint);

    let output = quote! {
        /// Helper function to join URL segments properly
        fn join_url_segments(base: &str, path: &str) -> String {
            let base = base.trim_end_matches('/');
            let path = path.trim_start_matches('/');
            if path.is_empty() {
                base.to_string()
            } else {
                format!("{}/{}", base, path)
            }
        }

        /// Generated client for the REST service
        #[derive(Clone)]
        pub struct #client_name {
            client: reqwest::Client,
            server_url: String,
            base_path: String,
            bearer_token: Option<String>,
            default_timeout: Option<std::time::Duration>,
        }

        /// Builder for the REST client
        pub struct #client_builder_name {
            server_url: String,
            timeout: Option<std::time::Duration>,
        }

        impl #client_builder_name {
            /// Create a new client builder with the required server URL
            pub fn new(server_url: impl Into<String>) -> Self {
                Self {
                    server_url: server_url.into(),
                    timeout: None,
                }
            }

            /// Set the default timeout for requests
            pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            /// Build the client
            ///
            /// # Errors
            ///
            /// Returns an error if the underlying HTTP client fails to build
            pub fn build(self) -> Result<#client_name, Box<dyn std::error::Error + Send + Sync>> {
                let mut client_builder = reqwest::Client::builder();

                // Timeout is not supported in WASM builds
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(timeout) = self.timeout {
                    client_builder = client_builder.timeout(timeout);
                }

                let client = client_builder.build()?;

                Ok(#client_name {
                    client,
                    server_url: self.server_url,
                    base_path: #base_path.to_string(),
                    bearer_token: None,
                    default_timeout: self.timeout,
                })
            }

            pub fn build_with_client_builder(self, mut client_builder: ::reqwest::ClientBuilder) -> Result<#client_name, Box<dyn std::error::Error + Send + Sync>> {
                // Timeout is not supported in WASM builds
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(timeout) = self.timeout {
                    client_builder = client_builder.timeout(timeout);
                }

                let client = client_builder.build()?;

                Ok(#client_name {
                    client,
                    server_url: self.server_url,
                    base_path: #base_path.to_string(),
                    bearer_token: None,
                    default_timeout: self.timeout,
                })
            }
        }

        impl #client_name {
            /// Set the bearer token for authentication
            pub fn set_bearer_token(&mut self, token: Option<impl Into<String>>) {
                self.bearer_token = token.map(|t| t.into());
            }

            /// Get a reference to the bearer token
            pub fn bearer_token(&self) -> Option<&str> {
                self.bearer_token.as_deref()
            }

            /// Create a new client builder
            pub fn builder(server_url: impl Into<String>) -> #client_builder_name {
                #client_builder_name::new(server_url)
            }

            #(#client_methods)*
            #(#client_methods_with_timeout)*
        }
    };

    output
}

fn handler_name_for_path(method: &HttpMethod, path: &str) -> syn::Ident {
    let method_str = method.as_str().to_lowercase();
    let mut parts = Vec::new();

    for segment in path.trim_start_matches('/').split('/') {
        if segment.starts_with('{') && segment.ends_with('}') {
            let inner = &segment[1..segment.len() - 1];
            let name = inner.split(':').next().unwrap_or(inner).trim();
            parts.push(format!("by_{name}"));
        } else if !segment.is_empty() {
            parts.push(segment.to_string());
        }
    }

    syn::parse_str::<syn::Ident>(&format!("{}_{}", method_str, parts.join("_")))
        .expect("generated REST client method name must be a valid Rust identifier")
}

fn generate_client_methods_for_endpoint(
    endpoint: &EndpointDefinition,
) -> Vec<proc_macro2::TokenStream> {
    let mut methods = vec![generate_client_method(
        &endpoint.handler_name,
        &endpoint.path_params,
        &endpoint.query_params,
        endpoint.request_type.as_ref(),
        &endpoint.response_type,
    )];

    methods.extend(endpoint.versions.iter().map(|version| {
        let method_name = handler_name_for_path(&endpoint.method, &version.path);
        generate_client_method(
            &method_name,
            &version.path_params,
            &version.query_params,
            version.request_type.as_ref(),
            &version.response_type,
        )
    }));

    methods
}

fn generate_client_methods_with_timeout_for_endpoint(
    endpoint: &EndpointDefinition,
) -> Vec<proc_macro2::TokenStream> {
    let mut methods = vec![generate_client_method_with_timeout(
        &endpoint.handler_name,
        &endpoint.method,
        &endpoint.path,
        &endpoint.path_params,
        &endpoint.query_params,
        endpoint.request_type.as_ref(),
        &endpoint.response_type,
    )];

    methods.extend(endpoint.versions.iter().map(|version| {
        let method_name = handler_name_for_path(&endpoint.method, &version.path);
        generate_client_method_with_timeout(
            &method_name,
            &endpoint.method,
            &version.path,
            &version.path_params,
            &version.query_params,
            version.request_type.as_ref(),
            &version.response_type,
        )
    }));

    methods
}

/// Generate a client method for the REST service
fn generate_client_method(
    method_name: &syn::Ident,
    path_params: &[crate::PathParam],
    query_params: &[crate::QueryParam],
    request_type: Option<&Type>,
    response_type: &Type,
) -> proc_macro2::TokenStream {
    // Build function parameters and call arguments
    let mut params = Vec::new();
    let mut call_args = Vec::new();

    // Add path parameters
    for path_param in path_params.iter() {
        let param_name = &path_param.name;
        let param_type = &path_param.param_type;
        params.push(quote! { #param_name: #param_type });
        call_args.push(quote! { #param_name });
    }

    // Add query parameters (mirroring the macro syntax order: path → query → body).
    for query_param in query_params.iter() {
        let param_name = &query_param.name;
        let param_type = &query_param.param_type;
        params.push(quote! { #param_name: #param_type });
        call_args.push(quote! { #param_name });
    }

    // Add request body parameter if present
    if let Some(request_type) = request_type {
        params.push(quote! { body: #request_type });
        call_args.push(quote! { body });
    }

    let method_name_with_timeout = quote::format_ident!("{}_with_timeout", method_name);

    quote! {
        /// Call the #method_name endpoint
        pub async fn #method_name(&self, #(#params),*) -> Result<#response_type, Box<dyn std::error::Error + Send + Sync>> {
            self.#method_name_with_timeout(#(#call_args,)* None).await
        }
    }
}

/// Generate a client method with timeout for the REST service
fn generate_client_method_with_timeout(
    method_name: &syn::Ident,
    method: &HttpMethod,
    path: &str,
    path_params: &[crate::PathParam],
    query_params: &[crate::QueryParam],
    request_type: Option<&Type>,
    response_type: &Type,
) -> proc_macro2::TokenStream {
    let method_name_with_timeout = quote::format_ident!("{}_with_timeout", method_name);
    let http_method = match method {
        HttpMethod::Get => quote! { reqwest::Method::GET },
        HttpMethod::Post => quote! { reqwest::Method::POST },
        HttpMethod::Put => quote! { reqwest::Method::PUT },
        HttpMethod::Delete => quote! { reqwest::Method::DELETE },
        HttpMethod::Patch => quote! { reqwest::Method::PATCH },
    };

    // Build function parameters
    let mut params = Vec::new();
    let mut path_substitutions = Vec::new();
    let mut param_names = Vec::new();
    // Build URL construction with proper joining
    let mut url_construction = quote! {
        join_url_segments(&join_url_segments(&self.server_url, &self.base_path), #path)
    };

    // Add path parameters
    for path_param in path_params.iter() {
        let param_name = &path_param.name;
        let param_type = &path_param.param_type;
        params.push(quote! { #param_name: #param_type });
        param_names.push(param_name);

        // Handle path parameter substitution
        let placeholder = format!("{{{}}}", param_name);
        path_substitutions.push(quote! {
            .replace(#placeholder, &#param_name.to_string())
        });
    }

    // Update URL construction if there are path parameters
    if !path_substitutions.is_empty() {
        url_construction = quote! {
            join_url_segments(&join_url_segments(&self.server_url, &self.base_path), #path)
            #(#path_substitutions)*
        };
    }

    // Build query-string handling. Required params are always serialized;
    // `Option<T>` params are skipped when `None`. Values are serialized by
    // reqwest's serde-backed `.query()` helper so enum serde renames and other
    // query wire formats stay aligned with server-side extraction.
    let query_handling = if query_params.is_empty() {
        quote! {}
    } else {
        let query_serializers = query_params.iter().map(|qp| {
            let param_name = &qp.name;
            let param_str = qp.name.to_string();
            if option_vec_inner_type(&qp.param_type).is_some() {
                quote! {
                    if let Some(__values) = &#param_name {
                        for __item in __values {
                            request_builder = request_builder.query(&[(#param_str, __item)]);
                        }
                    }
                }
            } else if vec_inner_type(&qp.param_type).is_some() {
                quote! {
                    for __item in &#param_name {
                        request_builder = request_builder.query(&[(#param_str, __item)]);
                    }
                }
            } else if option_inner_type(&qp.param_type).is_some() {
                quote! {
                    if let Some(__v) = &#param_name {
                        request_builder = request_builder.query(&[(#param_str, __v)]);
                    }
                }
            } else {
                quote! {
                    request_builder = request_builder.query(&[(#param_str, &#param_name)]);
                }
            }
        });
        quote! {
            #(#query_serializers)*
        }
    };

    // Add query parameters to the function signature (after path params,
    // before the body — matches macro syntax order).
    for query_param in query_params.iter() {
        let param_name = &query_param.name;
        let param_type = &query_param.param_type;
        params.push(quote! { #param_name: #param_type });
    }

    // Add request body parameter if present
    let request_body_handling = if let Some(request_type) = request_type {
        params.push(quote! { body: #request_type });
        quote! {
            request_builder = request_builder.json(&body);
        }
    } else {
        quote! {}
    };

    // Check if response type is unit type ()
    let is_unit_type = quote!(#response_type).to_string() == "()";

    let response_handling = if is_unit_type {
        quote! {
            if response.status().is_success() {
                Ok(())
            } else {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                Err(format!("HTTP error {}: {}", status, error_text).into())
            }
        }
    } else {
        quote! {
            if response.status().is_success() {
                let result = response.json().await?;
                Ok(result)
            } else {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                Err(format!("HTTP error {}: {}", status, error_text).into())
            }
        }
    };

    quote! {
        /// Call the #method_name endpoint with a custom timeout
        pub async fn #method_name_with_timeout(
            &self,
            #(#params,)*
            timeout: Option<std::time::Duration>
        ) -> Result<#response_type, Box<dyn std::error::Error + Send + Sync>> {
            let url = #url_construction;

            let mut request_builder = self.client
                .request(#http_method, &url);

            // Add bearer token if available
            if let Some(token) = &self.bearer_token {
                request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
            }

            #query_handling

            #request_body_handling

            // Override timeout if provided (not supported in WASM builds)
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(timeout) = timeout {
                request_builder = request_builder.timeout(timeout);
            }

            let response = request_builder.send().await?;

            #response_handling
        }
    }
}
