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

    quote! {
        #[derive(Clone)]
        pub struct #client_name {
            transport: ::std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            base_url: String,
            bearer_token: ::std::sync::Arc<::std::sync::RwLock<Option<String>>>,
            #[cfg(not(target_arch = "wasm32"))]
            default_timeout: Option<::std::time::Duration>,
        }

        impl #client_name {
            pub fn builder(base_url: impl Into<String>) -> #builder_name {
                #builder_name::new(base_url)
            }

            pub fn set_bearer_token(&self, token: Option<impl Into<String>>) {
                *self.bearer_token.write().unwrap() = token.map(|token| token.into());
            }

            fn build_request(
                &self,
                path: &str,
            ) -> (String, ::ras_transport_core::http::HeaderMap) {
                let base = self.base_url.trim_end_matches('/');
                let path = path.trim_start_matches('/');
                let url = format!("{}/{}", base, path);

                let mut headers = ::ras_transport_core::http::HeaderMap::new();
                if let Some(token) = self.bearer_token.read().unwrap().as_ref() {
                    if let Ok(value) = ::ras_transport_core::http::HeaderValue::from_str(
                        &format!("Bearer {}", token),
                    ) {
                        headers.insert(::ras_transport_core::http::header::AUTHORIZATION, value);
                    }
                }

                (url, headers)
            }

            #(#client_methods)*
        }

        pub struct #builder_name {
            base_url: String,
            transport: Option<::std::sync::Arc<dyn ::ras_transport_core::HttpTransport>>,
            #[cfg(not(target_arch = "wasm32"))]
            timeout: Option<::std::time::Duration>,
        }

        impl #builder_name {
            pub fn new(base_url: impl Into<String>) -> Self {
                Self {
                    base_url: base_url.into(),
                    transport: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    timeout: None,
                }
            }

            pub fn with_transport(
                mut self,
                transport: ::std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            ) -> Self {
                self.transport = Some(transport);
                self
            }

            #[cfg(not(target_arch = "wasm32"))]
            pub fn with_timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            pub fn build(self) -> #client_name {
                let transport: ::std::sync::Arc<dyn ::ras_transport_core::HttpTransport> =
                    match self.transport {
                        Some(transport) => transport,
                        None => ::std::sync::Arc::new(
                            ::ras_transport_core::ReqwestTransport::new(),
                        ),
                    };

                #client_name {
                    transport,
                    base_url: self.base_url,
                    bearer_token: ::std::sync::Arc::new(::std::sync::RwLock::new(None)),
                    #[cfg(not(target_arch = "wasm32"))]
                    default_timeout: self.timeout,
                }
            }

            pub fn build_with_transport(
                self,
                transport: ::std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            ) -> #client_name {
                #client_name {
                    transport,
                    base_url: self.base_url,
                    bearer_token: ::std::sync::Arc::new(::std::sync::RwLock::new(None)),
                    #[cfg(not(target_arch = "wasm32"))]
                    default_timeout: self.timeout,
                }
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
    let path = endpoint.path.value();
    let full_path = format!("{}{}", base_path.trim_end_matches('/'), path);
    let path_params = endpoint.path_params.iter().map(|param| {
        let name = &param.name;
        let ty = &param.ty;
        quote! { #name: #ty }
    });
    let path_replace = endpoint.path_params.iter().map(|param| {
        let name = &param.name;
        let placeholder = format!("{{{}}}", name);
        quote! { .replace(#placeholder, &#name.to_string()) }
    });

    match &endpoint.operation {
        Operation::Upload { response_type, .. } => {
            let form_builder = multipart_builder_name(definition, endpoint);
            quote! {
                pub async fn #method_name(
                    &self,
                    #(#path_params,)*
                    form: #form_builder,
                ) -> Result<#response_type, ::ras_transport_core::TransportError> {
                    let path = #full_path.to_string()#(#path_replace)*;
                    let (url, mut headers) = self.build_request(&path);

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

                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(timeout) = self.default_timeout {
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
                let path = #full_path.to_string()#(#path_replace)*;
                let (url, headers) = self.build_request(&path);

                let mut request = ::ras_transport_core::TransportRequest::new(
                    ::ras_transport_core::http::Method::GET,
                    url,
                );
                request.headers = headers;

                #[cfg(not(target_arch = "wasm32"))]
                if let Some(timeout) = self.default_timeout {
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
                    use ::futures_util::StreamExt as _;

                    let content_type = content_type.unwrap_or("application/octet-stream");
                    let path_ref = file_path.as_ref();

                    let builder = match file_name {
                        Some(name) => {
                            let file = ::tokio::fs::File::open(path_ref).await.map_err(|e| {
                                ::ras_transport_core::TransportError::Body(e.to_string())
                            })?;
                            let stream = ::ras_transport_core::byte_stream_from(
                                ::tokio_util::io::ReaderStream::new(file).map(|chunk| {
                                    chunk.map_err(|e| {
                                        ::ras_transport_core::TransportError::Body(e.to_string())
                                    })
                                }),
                            );
                            self.builder.stream_part(
                                #field_name,
                                name.to_string(),
                                content_type.to_string(),
                                stream,
                            )
                        }
                        None => self
                            .builder
                            .file_path(#field_name, content_type.to_string(), path_ref)
                            .await?,
                    };

                    self.builder = builder;
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
