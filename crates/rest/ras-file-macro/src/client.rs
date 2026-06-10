use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::parser::{Endpoint, FileServiceDefinition, Operation, UploadPartKind};

pub fn generate_client(definition: &FileServiceDefinition) -> TokenStream {
    let service_name = &definition.service_name;
    let base_path = definition.base_path.value();

    let client_name = format_ident!("{}Client", service_name);
    let builder_name = format_ident!("{}ClientBuilder", service_name);
    let form_builders = definition
        .endpoints
        .iter()
        .filter_map(|endpoint| generate_multipart_builder(definition, endpoint));
    let client_methods = definition
        .endpoints
        .iter()
        .map(|endpoint| generate_client_method(definition, endpoint, &base_path));
    // With `feature_gated: true` the convenience constructor is gated on the
    // CONSUMER crate's `reqwest` feature instead of the macro crate's
    // (workspace-unified) one.
    let cfg_reqwest = if definition.feature_gated {
        quote! { #[cfg(feature = "reqwest")] }
    } else {
        quote! {}
    };
    let build_method = if definition.feature_gated || cfg!(feature = "reqwest") {
        quote! {
            #cfg_reqwest
            pub fn build(
                self,
            ) -> Result<#client_name, Box<dyn ::std::error::Error + Send + Sync>> {
                let transport = ::std::sync::Arc::new(
                    ::ras_transport_core::ReqwestTransport::new(),
                );
                self.build_with_transport(transport)
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #[derive(Clone)]
        pub struct #client_name {
            transport: ::std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            base_url: String,
            bearer_token: Option<String>,
            default_timeout: Option<::std::time::Duration>,
        }

        impl #client_name {
            pub fn builder(base_url: impl Into<String>) -> #builder_name {
                #builder_name::new(base_url)
            }

            pub fn set_bearer_token(&mut self, token: Option<impl Into<String>>) {
                self.bearer_token = token.map(|token| token.into());
            }

            pub fn bearer_token(&self) -> Option<&str> {
                self.bearer_token.as_deref()
            }

            fn build_request(
                &self,
                path: &str,
            ) -> Result<
                (String, ::ras_transport_core::http::HeaderMap),
                ::ras_transport_core::TransportError,
            > {
                let base = self.base_url.trim_end_matches('/');
                let path = path.trim_start_matches('/');
                let url = format!("{}/{}", base, path);

                let mut headers = ::ras_transport_core::http::HeaderMap::new();
                if let Some(token) = self.bearer_token.as_ref() {
                    // Fail closed: a token that cannot be encoded must not be
                    // silently dropped, which would send the request
                    // unauthenticated.
                    let value = ::ras_transport_core::http::HeaderValue::from_str(
                        &format!("Bearer {}", token),
                    )
                    .map_err(|_| {
                        ::ras_transport_core::TransportError::InvalidHeader(
                            "bearer token".to_string(),
                        )
                    })?;
                    headers.insert(::ras_transport_core::http::header::AUTHORIZATION, value);
                }

                Ok((url, headers))
            }

            #(#client_methods)*
        }

        pub struct #builder_name {
            base_url: String,
            timeout: Option<::std::time::Duration>,
        }

        impl #builder_name {
            pub fn new(base_url: impl Into<String>) -> Self {
                Self {
                    base_url: base_url.into(),
                    timeout: None,
                }
            }

            pub fn with_timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            #build_method

            pub fn build_with_transport(
                self,
                transport: ::std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            ) -> Result<#client_name, Box<dyn ::std::error::Error + Send + Sync>> {
                Ok(#client_name {
                    transport,
                    base_url: self.base_url,
                    bearer_token: None,
                    default_timeout: self.timeout,
                })
            }
        }

        #(#form_builders)*
    }
}

fn generate_client_method(
    definition: &FileServiceDefinition,
    endpoint: &Endpoint,
    base_path: &str,
) -> TokenStream {
    let method_name = &endpoint.name;
    let method_name_with_timeout = format_ident!("{}_with_timeout", method_name);
    let method_name_with_optional_timeout =
        format_ident!("__ras_{}_with_optional_timeout", method_name);
    let path = endpoint.path.value();
    let full_path = format!("{}{}", base_path.trim_end_matches('/'), path);
    let path_params: Vec<_> = endpoint
        .path_params
        .iter()
        .map(|param| {
            let name = &param.name;
            let ty = &param.ty;
            quote! { #name: #ty }
        })
        .collect();
    let path_args: Vec<_> = endpoint
        .path_params
        .iter()
        .map(|param| {
            let name = &param.name;
            quote! { #name }
        })
        .collect();
    // Percent-encode each path parameter for its segment so a `/`, `?`, `#`,
    // etc. in a caller-supplied value cannot escape its slot and alter the
    // request's path or query.
    let path_replace: Vec<_> = endpoint
        .path_params
        .iter()
        .map(|param| {
            let name = &param.name;
            let placeholder = format!("{{{}}}", name);
            quote! {
                .replace(
                    #placeholder,
                    &::ras_transport_core::encode_path_segment(&#name.to_string()),
                )
            }
        })
        .collect();

    match &endpoint.operation {
        Operation::Upload { response_type, .. } => {
            let form_builder = multipart_builder_name(definition, endpoint);
            quote! {
                pub async fn #method_name(
                    &self,
                    #(#path_params,)*
                    form: #form_builder,
                ) -> Result<#response_type, ::ras_transport_core::TransportError> {
                    self.#method_name_with_optional_timeout(#(#path_args,)* form, None).await
                }

                pub async fn #method_name_with_timeout(
                    &self,
                    #(#path_params,)*
                    form: #form_builder,
                    timeout: ::std::time::Duration,
                ) -> Result<#response_type, ::ras_transport_core::TransportError> {
                    self.#method_name_with_optional_timeout(#(#path_args,)* form, Some(timeout)).await
                }

                async fn #method_name_with_optional_timeout(
                    &self,
                    #(#path_params,)*
                    form: #form_builder,
                    timeout: Option<::std::time::Duration>,
                ) -> Result<#response_type, ::ras_transport_core::TransportError> {
                    let path = #full_path.to_string()#(#path_replace)*;
                    let (url, mut headers) = self.build_request(&path)?;

                    let (body, content_type) = form.into_body();
                    if let Ok(value) = ::ras_transport_core::http::HeaderValue::from_str(&content_type) {
                        headers.insert(::ras_transport_core::http::header::CONTENT_TYPE, value);
                    }

                    let mut request = ::ras_transport_core::TransportRequest::new(
                        ::ras_transport_core::http::Method::POST,
                        url,
                    )
                    .body(body);
                    request.headers = headers;

                    if let Some(timeout) = timeout.or(self.default_timeout) {
                        request = request.timeout(timeout);
                    }

                    let response = self.transport.execute(request).await?.error_for_status().await?;
                    response.json().await
                }
            }
        }
        Operation::Download { .. } => quote! {
            pub async fn #method_name(
                &self,
                #(#path_params,)*
            ) -> Result<::ras_transport_core::TransportResponse, ::ras_transport_core::TransportError> {
                self.#method_name_with_optional_timeout(#(#path_args,)* None).await
            }

            pub async fn #method_name_with_timeout(
                &self,
                #(#path_params,)*
                timeout: ::std::time::Duration,
            ) -> Result<::ras_transport_core::TransportResponse, ::ras_transport_core::TransportError> {
                self.#method_name_with_optional_timeout(#(#path_args,)* Some(timeout)).await
            }

            async fn #method_name_with_optional_timeout(
                &self,
                #(#path_params,)*
                timeout: Option<::std::time::Duration>,
            ) -> Result<::ras_transport_core::TransportResponse, ::ras_transport_core::TransportError> {
                let path = #full_path.to_string()#(#path_replace)*;
                let (url, headers) = self.build_request(&path)?;

                let mut request = ::ras_transport_core::TransportRequest::new(
                    ::ras_transport_core::http::Method::GET,
                    url,
                );
                request.headers = headers;

                if let Some(timeout) = timeout.or(self.default_timeout) {
                    request = request.timeout(timeout);
                }

                let response = self.transport.execute(request).await?.error_for_status().await?;
                Ok(response)
            }
        },
    }
}

fn generate_multipart_builder(
    definition: &FileServiceDefinition,
    endpoint: &Endpoint,
) -> Option<TokenStream> {
    let Operation::Upload { config, .. } = &endpoint.operation else {
        return None;
    };

    let builder_name = multipart_builder_name(definition, endpoint);
    let methods = config.parts.iter().map(|part| {
        let field_name = part.name.to_string();
        let method_name = &part.name;
        let bytes_method_name = format_ident!("{}_bytes", method_name);

        match part.kind {
            UploadPartKind::File => quote! {
                #[cfg(all(not(target_arch = "wasm32"), feature = "fs"))]
                pub async fn #method_name(
                    mut self,
                    file_path: impl AsRef<std::path::Path>,
                    file_name: Option<&str>,
                    content_type: Option<&str>,
                ) -> Result<Self, ::ras_transport_core::TransportError> {
                    let content_type = content_type.unwrap_or("application/octet-stream");
                    // The disk -> stream conversion (and its tokio/tokio-util/
                    // futures-util usage) lives entirely in ras-transport-core,
                    // so consumers need not depend on those crates.
                    self.builder = self
                        .builder
                        .file_path(
                            #field_name,
                            file_name.map(|name| name.to_string()),
                            content_type.to_string(),
                            file_path.as_ref(),
                        )
                        .await?;
                    Ok(self)
                }

                pub fn #bytes_method_name(
                    mut self,
                    bytes: impl Into<Vec<u8>>,
                    file_name: impl Into<String>,
                    content_type: Option<&str>,
                ) -> Result<Self, ::ras_transport_core::TransportError> {
                    let content_type = content_type.unwrap_or("application/octet-stream");
                    let bytes: Vec<u8> = bytes.into();
                    self.builder = self.builder.bytes_part(
                        #field_name,
                        file_name.into(),
                        content_type.to_string(),
                        bytes,
                    );
                    Ok(self)
                }
            },
            UploadPartKind::Json => {
                let ty = part.ty.as_ref().expect("json part type");
                quote! {
                    pub fn #method_name(
                        mut self,
                        value: &#ty,
                    ) -> Result<Self, ::ras_transport_core::TransportError> {
                        self.builder = self.builder.json(#field_name, value)?;
                        Ok(self)
                    }
                }
            }
            UploadPartKind::Text => quote! {
                pub fn #method_name(mut self, value: impl Into<String>) -> Self {
                    self.builder = self.builder.text(#field_name, value.into());
                    self
                }
            },
        }
    });

    Some(quote! {
        pub struct #builder_name {
            builder: ::ras_transport_core::MultipartBuilder,
        }

        impl #builder_name {
            pub fn new() -> Self {
                Self {
                    builder: ::ras_transport_core::MultipartBuilder::new(),
                }
            }

            #(#methods)*

            pub fn into_body(self) -> (::ras_transport_core::RequestBody, String) {
                self.builder.build()
            }
        }

        impl Default for #builder_name {
            fn default() -> Self {
                Self::new()
            }
        }
    })
}

fn multipart_builder_name(
    definition: &FileServiceDefinition,
    endpoint: &Endpoint,
) -> proc_macro2::Ident {
    format_ident!(
        "{}{}Multipart",
        definition.service_name,
        pascal_ident_segment(&endpoint.name.to_string())
    )
}

fn pascal_ident_segment(value: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = true;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                out.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                out.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }

    if out.is_empty() {
        "Generated".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("V{out}")
    } else {
        out
    }
}
