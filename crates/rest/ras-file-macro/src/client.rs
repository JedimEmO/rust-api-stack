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
        pub struct #client_name {
            client: ::reqwest::Client,
            base_url: String,
            bearer_token: ::std::sync::RwLock<Option<String>>,
        }

        impl #client_name {
            pub fn builder(base_url: impl Into<String>) -> #builder_name {
                #builder_name::new(base_url)
            }

            pub fn set_bearer_token(&self, token: Option<impl Into<String>>) {
                *self.bearer_token.write().unwrap() = token.map(|token| token.into());
            }

            fn build_request(&self, method: ::reqwest::Method, path: &str) -> ::reqwest::RequestBuilder {
                let base = self.base_url.trim_end_matches('/');
                let path = path.trim_start_matches('/');
                let mut request = self.client.request(method, format!("{}/{}", base, path));

                if let Some(token) = self.bearer_token.read().unwrap().as_ref() {
                    request = request.header("Authorization", format!("Bearer {}", token));
                }

                request
            }

            #(#client_methods)*
        }

        pub struct #builder_name {
            base_url: String,
            client: Option<::reqwest::Client>,
            #[cfg(not(target_arch = "wasm32"))]
            timeout: Option<std::time::Duration>,
        }

        impl #builder_name {
            pub fn new(base_url: impl Into<String>) -> Self {
                Self {
                    base_url: base_url.into(),
                    client: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    timeout: None,
                }
            }

            pub fn with_client(mut self, client: ::reqwest::Client) -> Self {
                self.client = Some(client);
                self
            }

            #[cfg(not(target_arch = "wasm32"))]
            pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            pub fn build(self) -> Result<#client_name, Box<dyn std::error::Error + Send + Sync>> {
                let client = match self.client {
                    Some(client) => client,
                    None => {
                        let mut builder = ::reqwest::Client::builder();

                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(timeout) = self.timeout {
                            builder = builder.timeout(timeout);
                        }

                        builder.build()?
                    }
                };

                Ok(#client_name {
                    client,
                    base_url: self.base_url,
                    bearer_token: ::std::sync::RwLock::new(None),
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
                ) -> Result<#response_type, Box<dyn std::error::Error + Send + Sync>> {
                    let path = #full_path.to_string()#(#path_replace)*;
                    let response = self
                        .build_request(::reqwest::Method::POST, &path)
                        .multipart(form.into_form())
                        .send()
                        .await?;

                    if !response.status().is_success() {
                        let status = response.status();
                        let text = response.text().await?;
                        return Err(format!("Upload failed with status {}: {}", status, text).into());
                    }

                    Ok(response.json().await?)
                }
            }
        }
        Operation::Download { .. } => quote! {
            pub async fn #method_name(
                &self,
                #(#path_params,)*
            ) -> Result<::reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
                let path = #full_path.to_string()#(#path_replace)*;
                let response = self
                    .build_request(::reqwest::Method::GET, &path)
                    .send()
                    .await?;

                if !response.status().is_success() {
                    let status = response.status();
                    let text = response.text().await?;
                    return Err(format!("Download failed with status {}: {}", status, text).into());
                }

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
                #[cfg(not(target_arch = "wasm32"))]
                pub async fn #method_name(
                    mut self,
                    file_path: impl AsRef<std::path::Path>,
                    file_name: Option<&str>,
                    content_type: Option<&str>,
                ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                    let file = ::tokio::fs::File::open(file_path.as_ref()).await?;
                    let length = file.metadata().await.ok().map(|metadata| metadata.len());
                    let stream = ::tokio_util::io::ReaderStream::new(file);
                    let body = ::reqwest::Body::wrap_stream(stream);

                    let file_name = file_name
                        .map(ToString::to_string)
                        .or_else(|| {
                            file_path
                                .as_ref()
                                .file_name()
                                .and_then(|name| name.to_str())
                                .map(ToString::to_string)
                        })
                        .unwrap_or_else(|| "file".to_string());

                    let mut part = if let Some(length) = length {
                        ::reqwest::multipart::Part::stream_with_length(body, length)
                    } else {
                        ::reqwest::multipart::Part::stream(body)
                    }
                    .file_name(file_name);

                    if let Some(content_type) = content_type {
                        part = part.mime_str(content_type)?;
                    }

                    self.form = self.form.part(#field_name, part);
                    Ok(self)
                }

                pub fn #bytes_method_name(
                    mut self,
                    bytes: impl Into<Vec<u8>>,
                    file_name: impl Into<String>,
                    content_type: Option<&str>,
                ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                    let mut part = ::reqwest::multipart::Part::bytes(bytes.into())
                        .file_name(file_name.into());
                    if let Some(content_type) = content_type {
                        part = part.mime_str(content_type)?;
                    }
                    self.form = self.form.part(#field_name, part);
                    Ok(self)
                }
            },
            UploadPartKind::Json => {
                let ty = part.ty.as_ref().expect("json part type");
                quote! {
                    pub fn #method_name(
                        mut self,
                        value: &#ty,
                    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
                        let json = ::serde_json::to_string(value)?;
                        let part = ::reqwest::multipart::Part::text(json)
                            .mime_str("application/json")?;
                        self.form = self.form.part(#field_name, part);
                        Ok(self)
                    }
                }
            }
            UploadPartKind::Text => quote! {
                pub fn #method_name(mut self, value: impl Into<String>) -> Self {
                    self.form = self.form.part(#field_name, ::reqwest::multipart::Part::text(value.into()));
                    self
                }
            },
        }
    });

    Some(quote! {
        pub struct #builder_name {
            form: ::reqwest::multipart::Form,
        }

        impl #builder_name {
            pub fn new() -> Self {
                Self {
                    form: ::reqwest::multipart::Form::new(),
                }
            }

            #(#methods)*

            pub fn into_form(self) -> ::reqwest::multipart::Form {
                self.form
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
