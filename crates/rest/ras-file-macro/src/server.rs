use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use crate::parser::{
    AuthRequirement, Endpoint, FileServiceDefinition, FilenamePolicy, MaxBytes, Operation,
    PathParam, UploadConfig, UploadPart, UploadPartKind,
};

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

        fn __ras_file_multipart_error(error: ::axum::extract::multipart::MultipartError) -> ::ras_file_core::FileError {
            if error.status() == ::axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                ::ras_file_core::FileError::PayloadTooLarge
            } else {
                ::ras_file_core::FileError::bad_request(error.body_text())
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

fn generate_support_types(definition: &FileServiceDefinition) -> TokenStream {
    let support = definition.endpoints.iter().flat_map(|endpoint| {
        let path_struct = path_struct_name(&definition.service_name, endpoint);
        let path_fields = endpoint.path_params.iter().map(|param| {
            let name = &param.name;
            let ty = &param.ty;
            quote! { pub #name: #ty }
        });

        let mut tokens = vec![quote! {
            #[derive(Debug, Clone)]
            pub struct #path_struct {
                #(#path_fields),*
            }
        }];

        if let Operation::Upload { config, .. } = &endpoint.operation {
            let part_enum = part_enum_name(&definition.service_name, endpoint);
            let has_file_part = config
                .parts
                .iter()
                .any(|part| part.kind == UploadPartKind::File);
            let variants = config.parts.iter().map(|part| {
                let variant = part_variant_name(part);
                match part.kind {
                    UploadPartKind::File => quote! { #variant(::ras_file_core::IncomingFile<'a>) },
                    UploadPartKind::Json => {
                        let ty = part.ty.as_ref().expect("json part type");
                        quote! { #variant(#ty) }
                    }
                    UploadPartKind::Text => quote! { #variant(String) },
                }
            });
            let lifetime_variant = if has_file_part {
                quote! {}
            } else {
                quote! { #[doc(hidden)] __Lifetime(std::marker::PhantomData<&'a ()>), }
            };

            let consumed_arms = config.parts.iter().map(|part| {
                let variant = part_variant_name(part);
                match part.kind {
                    UploadPartKind::File => quote! { Self::#variant(file) => file.is_finished() },
                    UploadPartKind::Json | UploadPartKind::Text => {
                        quote! { Self::#variant(_) => true }
                    }
                }
            });
            let lifetime_consumed_arm = if has_file_part {
                quote! {}
            } else {
                quote! { Self::__Lifetime(_) => true, }
            };

            let bytes_arms = config.parts.iter().map(|part| {
                let variant = part_variant_name(part);
                match part.kind {
                    UploadPartKind::File => quote! { Self::#variant(file) => file.bytes_read() },
                    UploadPartKind::Json | UploadPartKind::Text => {
                        quote! { Self::#variant(_) => 0 }
                    }
                }
            });
            let lifetime_bytes_arm = if has_file_part {
                quote! {}
            } else {
                quote! { Self::__Lifetime(_) => 0, }
            };

            tokens.push(quote! {
                pub enum #part_enum<'a> {
                    #lifetime_variant
                    #(#variants),*
                }

                impl #part_enum<'_> {
                    pub fn is_consumed(&self) -> bool {
                        match self {
                            #lifetime_consumed_arm
                            #(#consumed_arms),*
                        }
                    }

                    pub fn bytes_read(&self) -> u64 {
                        match self {
                            #lifetime_bytes_arm
                            #(#bytes_arms),*
                        }
                    }
                }
            });
        }

        tokens
    });

    quote! { #(#support)* }
}

fn generate_trait_methods(definition: &FileServiceDefinition, _trait_name: &Ident) -> TokenStream {
    let methods = definition.endpoints.iter().map(|endpoint| {
        let path_struct = path_struct_name(&definition.service_name, endpoint);
        let handler_name = &endpoint.name;

        match &endpoint.operation {
            Operation::Upload { response_type, .. } => {
                let state_type = upload_state_type_name(endpoint);
                let begin = format_ident!("{}_begin", handler_name);
                let part = format_ident!("{}_part", handler_name);
                let finish = format_ident!("{}_finish", handler_name);
                let abort = format_ident!("{}_abort", handler_name);
                let part_enum = part_enum_name(&definition.service_name, endpoint);

                quote! {
                    type #state_type: Send;

                    async fn #begin(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: &#path_struct,
                    ) -> ::ras_file_core::FileResult<Self::#state_type>;

                    async fn #part(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: &#path_struct,
                        state: &mut Self::#state_type,
                        part: &mut #part_enum<'_>,
                    ) -> ::ras_file_core::FileResult<()>;

                    async fn #finish(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: &#path_struct,
                        state: Self::#state_type,
                        summary: ::ras_file_core::UploadSummary,
                    ) -> ::ras_file_core::FileResult<::ras_file_core::JsonResponse<#response_type>>;

                    async fn #abort(
                        &self,
                        _ctx: &::ras_file_core::FileRequestContext<'_>,
                        _path: &#path_struct,
                        _state: Self::#state_type,
                        _error: &::ras_file_core::FileError,
                    ) {
                    }
                }
            }
            Operation::Download { .. } => {
                quote! {
                    async fn #handler_name(
                        &self,
                        ctx: &::ras_file_core::FileRequestContext<'_>,
                        path: #path_struct,
                    ) -> ::ras_file_core::FileResult<::ras_file_core::DownloadResponse>;
                }
            }
        }
    });

    quote! { #(#methods)* }
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

fn generate_upload_handler(
    definition: &FileServiceDefinition,
    endpoint: &Endpoint,
    config: &UploadConfig,
    trait_name: &Ident,
) -> TokenStream {
    let handler_fn = format_ident!("{}_handler", endpoint.name);
    let begin = format_ident!("{}_begin", endpoint.name);
    let part_method = format_ident!("{}_part", endpoint.name);
    let finish = format_ident!("{}_finish", endpoint.name);
    let abort = format_ident!("{}_abort", endpoint.name);
    let path = endpoint.path.value();
    let path_struct = path_struct_name(&definition.service_name, endpoint);
    let part_enum = part_enum_name(&definition.service_name, endpoint);
    let auth = generate_auth_check(&endpoint.auth);
    let permission_check = generate_permission_check(&endpoint.auth);
    let path_extraction = generate_path_extraction(&endpoint.path_params, &path_struct);
    let content_length_limit = match &config.max_total_bytes {
        MaxBytes::Limited(limit) => quote! {
            if let Some(content_length) = parts.headers
                .get(::axum::http::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
            {
                if content_length > #limit {
                    return __ras_file_error_response(::ras_file_core::FileError::PayloadTooLarge);
                }
            }
        },
        MaxBytes::Unlimited => quote! {},
    };
    let max_total_limit = match &config.max_total_bytes {
        MaxBytes::Limited(limit) => quote! { Some(#limit as u64) },
        MaxBytes::Unlimited => quote! { None },
    };
    let part_dispatch = generate_part_dispatch(config, &part_enum, &part_method, &abort);
    let required_checks = generate_required_checks(config, &abort);
    let part_count_vars = config.parts.iter().map(|part| {
        let count_ident = part_count_ident(part);
        quote! { let mut #count_ident: usize = 0; }
    });

    quote! {
        async fn #handler_fn<S, A>(
            state: ::axum::extract::State<(
                ::std::sync::Arc<S>,
                Option<::std::sync::Arc<A>>,
                Option<::std::sync::Arc<Box<dyn Fn(&::axum::http::HeaderMap, &str, &str) + Send + Sync>>>,
                Option<::std::sync::Arc<Box<dyn Fn(&str, &str, std::time::Duration) + Send + Sync>>>,
                ::ras_auth_core::AuthTransportConfig,
            )>,
            req: ::axum::http::Request<::axum::body::Body>,
        ) -> ::axum::response::Response
        where
            S: #trait_name + Send + Sync + 'static,
            A: ::ras_auth_core::AuthProvider + Send + Sync + 'static,
        {
            use ::axum::extract::FromRequest;
            use ::axum::response::IntoResponse;

            let start = std::time::Instant::now();
            let method = "POST";
            let request_path = req.uri().path().to_string();
            let (mut parts, body) = req.into_parts();

            if let Some(tracker) = &state.2 {
                let tracker_headers =
                    ::ras_auth_core::redact_sensitive_headers_for_auth_transport(&parts.headers, &state.4);
                tracker(&tracker_headers, method, &request_path);
            }

            #auth
            #permission_check
            #path_extraction

            #content_length_limit

            let content_type = parts.headers
                .get(::axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            if !content_type.starts_with("multipart/form-data") {
                return __ras_file_error_response(::ras_file_core::FileError::unsupported_media_type(
                    "expected multipart/form-data",
                ));
            }

            let request_headers = parts.headers.clone();
            let ctx = ::ras_file_core::FileRequestContext::new(
                method,
                &request_path,
                #path,
                &request_headers,
                user.as_ref(),
            );

            let req = ::axum::http::Request::from_parts(parts, body);
            let mut multipart = match <::axum::extract::Multipart as FromRequest<_>>::from_request(req, &state).await {
                Ok(multipart) => multipart,
                Err(rejection) => return rejection.into_response(),
            };

            let service = &state.0.0;
            let mut upload_state = Some(match service.#begin(&ctx, &path_value).await {
                Ok(upload_state) => upload_state,
                Err(error) => return __ras_file_error_response(error),
            });

            let mut summary = ::ras_file_core::UploadSummary::default();
            let mut total_bytes: u64 = 0;
            let max_total_bytes: Option<u64> = #max_total_limit;
            #(#part_count_vars)*

            while let Some(mut field) = match multipart.next_field().await {
                Ok(field) => field,
                Err(error) => {
                    let error = __ras_file_multipart_error(error);
                    let upload_state = upload_state.take().expect("upload state is present before abort");
                    service.#abort(&ctx, &path_value, upload_state, &error).await;
                    return __ras_file_error_response(error);
                }
            } {
                let field_name = field.name().unwrap_or("").to_string();
                #part_dispatch
            }

            #required_checks

            let upload_state = upload_state.take().expect("upload state is present before finish");
            let response = match service.#finish(&ctx, &path_value, upload_state, summary).await {
                Ok(response) => response,
                Err(error) => return __ras_file_error_response(error),
            };

            if let Some(tracker) = &state.3 {
                tracker(method, &request_path, start.elapsed());
            }

            let (status, headers, body) = response.into_parts();
            let mut response = (status, ::axum::Json(body)).into_response();
            response.headers_mut().extend(headers);
            response
        }
    }
}

fn generate_part_dispatch(
    config: &UploadConfig,
    part_enum: &Ident,
    part_method: &Ident,
    abort: &Ident,
) -> TokenStream {
    let arms = config
        .parts
        .iter()
        .map(|part| generate_part_arm(part, part_enum, part_method, abort));
    let unknown = if config.reject_unknown_fields {
        quote! {
            {
                let error = ::ras_file_core::FileError::bad_request(format!("unknown multipart field `{}`", field_name));
                let upload_state = upload_state.take().expect("upload state is present before abort");
                service.#abort(&ctx, &path_value, upload_state, &error).await;
                return __ras_file_error_response(error);
            }
        }
    } else {
        quote! {
            {
                let mut ignored_bytes: u64 = 0;
                loop {
                    let maybe_chunk = match field.chunk().await {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            let error = __ras_file_multipart_error(error);
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    };

                    let Some(chunk) = maybe_chunk else {
                        break;
                    };

                    ignored_bytes = ignored_bytes.saturating_add(chunk.len() as u64);
                    if let Some(max_total) = max_total_bytes {
                        if total_bytes.saturating_add(ignored_bytes) > max_total {
                            let error = ::ras_file_core::FileError::PayloadTooLarge;
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    }
                }
                total_bytes = total_bytes.saturating_add(ignored_bytes);
            }
        }
    };

    quote! {
        match field_name.as_str() {
            #(#arms,)*
            _ => #unknown,
        }
    }
}

fn generate_part_arm(
    part: &UploadPart,
    part_enum: &Ident,
    part_method: &Ident,
    abort: &Ident,
) -> TokenStream {
    let field_name = part.name.to_string();
    let count_ident = part_count_ident(part);
    let max_count = part.max_count;
    let max_bytes = part.max_bytes;
    let variant = part_variant_name(part);

    let content_type_check = if part.content_types.is_empty() {
        quote! {}
    } else {
        let allowed = part.content_types.iter();
        quote! {
            let content_type = field.content_type().unwrap_or("").to_string();
            if ![#(#allowed),*].contains(&content_type.as_str()) {
                let error = ::ras_file_core::FileError::unsupported_media_type(
                    format!("unsupported content type `{}` for field `{}`", content_type, #field_name),
                );
                let upload_state = upload_state.take().expect("upload state is present before abort");
                service.#abort(&ctx, &path_value, upload_state, &error).await;
                return __ras_file_error_response(error);
            }
        }
    };

    let count_check = quote! {
        if #count_ident >= #max_count {
            let error = ::ras_file_core::FileError::bad_request(format!("too many `{}` parts", #field_name));
            let upload_state = upload_state.take().expect("upload state is present before abort");
            service.#abort(&ctx, &path_value, upload_state, &error).await;
            return __ras_file_error_response(error);
        }
        #count_ident += 1;
    };

    match part.kind {
        UploadPartKind::File => {
            let filename_check = match part.filename {
                FilenamePolicy::Optional => quote! {},
                FilenamePolicy::Required => quote! {
                    if field.file_name().is_none() {
                        let error = ::ras_file_core::FileError::bad_request(format!("field `{}` requires a filename", #field_name));
                        let upload_state = upload_state.take().expect("upload state is present before abort");
                        service.#abort(&ctx, &path_value, upload_state, &error).await;
                        return __ras_file_error_response(error);
                    }
                },
                FilenamePolicy::Forbidden => quote! {
                    if field.file_name().is_some() {
                        let error = ::ras_file_core::FileError::bad_request(format!("field `{}` must not include a filename", #field_name));
                        let upload_state = upload_state.take().expect("upload state is present before abort");
                        service.#abort(&ctx, &path_value, upload_state, &error).await;
                        return __ras_file_error_response(error);
                    }
                },
            };

            quote! {
                #field_name => {
                    #count_check
                    #content_type_check
                    #filename_check

                    let remaining_total = max_total_bytes
                        .map(|max| max.saturating_sub(total_bytes))
                        .unwrap_or(u64::MAX);
                    let part_limit = std::cmp::min(#max_bytes as u64, remaining_total);
                    let file_name = field.file_name().map(ToString::to_string);
                    let content_type = field.content_type().map(ToString::to_string);
                    let headers = field.headers().clone();
                    let stream = ::ras_file_core::futures_util::StreamExt::map(field, |chunk| {
                        chunk.map_err(__ras_file_multipart_error)
                    });
                    let file = ::ras_file_core::IncomingFile::new(
                        #field_name,
                        file_name,
                        content_type,
                        headers,
                        part_limit,
                        Box::pin(stream),
                    );
                    let mut part = #part_enum::#variant(file);

                    let part_result = {
                        let upload_state = upload_state.as_mut().expect("upload state is present while handling parts");
                        service.#part_method(&ctx, &path_value, upload_state, &mut part).await
                    };
                    if let Err(error) = part_result {
                        let upload_state = upload_state.take().expect("upload state is present before abort");
                        service.#abort(&ctx, &path_value, upload_state, &error).await;
                        return __ras_file_error_response(error);
                    }

                    if !part.is_consumed() {
                        let error = ::ras_file_core::FileError::handler_contract(format!("handler did not consume file field `{}`", #field_name));
                        let upload_state = upload_state.take().expect("upload state is present before abort");
                        service.#abort(&ctx, &path_value, upload_state, &error).await;
                        return __ras_file_error_response(error);
                    }

                    let bytes_read = part.bytes_read();
                    if let Some(max_total) = max_total_bytes {
                        if total_bytes.saturating_add(bytes_read) > max_total {
                            let error = ::ras_file_core::FileError::PayloadTooLarge;
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    }
                    total_bytes = total_bytes.saturating_add(bytes_read);
                    summary.record(#field_name, bytes_read);
                }
            }
        }
        UploadPartKind::Json => {
            let ty = part.ty.as_ref().expect("json part type");
            quote! {
                #field_name => {
                    #count_check
                    #content_type_check
                    let bytes = match __ras_read_field_bytes(field, #max_bytes as u64, max_total_bytes.map(|max| max.saturating_sub(total_bytes))).await {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    };
                    let value: #ty = match ::serde_json::from_slice(&bytes) {
                        Ok(value) => value,
                        Err(error) => {
                            let error = ::ras_file_core::FileError::bad_request(format!("invalid JSON in field `{}`: {}", #field_name, error));
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    };
                    let mut part = #part_enum::#variant(value);
                    let part_result = {
                        let upload_state = upload_state.as_mut().expect("upload state is present while handling parts");
                        service.#part_method(&ctx, &path_value, upload_state, &mut part).await
                    };
                    if let Err(error) = part_result {
                        let upload_state = upload_state.take().expect("upload state is present before abort");
                        service.#abort(&ctx, &path_value, upload_state, &error).await;
                        return __ras_file_error_response(error);
                    }
                    total_bytes = total_bytes.saturating_add(bytes.len() as u64);
                    summary.record(#field_name, bytes.len() as u64);
                }
            }
        }
        UploadPartKind::Text => {
            quote! {
                #field_name => {
                    #count_check
                    #content_type_check
                    let bytes = match __ras_read_field_bytes(field, #max_bytes as u64, max_total_bytes.map(|max| max.saturating_sub(total_bytes))).await {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    };
                    let value = match String::from_utf8(bytes.to_vec()) {
                        Ok(value) => value,
                        Err(error) => {
                            let error = ::ras_file_core::FileError::bad_request(format!("invalid UTF-8 in field `{}`: {}", #field_name, error));
                            let upload_state = upload_state.take().expect("upload state is present before abort");
                            service.#abort(&ctx, &path_value, upload_state, &error).await;
                            return __ras_file_error_response(error);
                        }
                    };
                    let mut part = #part_enum::#variant(value);
                    let part_result = {
                        let upload_state = upload_state.as_mut().expect("upload state is present while handling parts");
                        service.#part_method(&ctx, &path_value, upload_state, &mut part).await
                    };
                    if let Err(error) = part_result {
                        let upload_state = upload_state.take().expect("upload state is present before abort");
                        service.#abort(&ctx, &path_value, upload_state, &error).await;
                        return __ras_file_error_response(error);
                    }
                    total_bytes = total_bytes.saturating_add(bytes.len() as u64);
                    summary.record(#field_name, bytes.len() as u64);
                }
            }
        }
    }
}

fn generate_required_checks(config: &UploadConfig, abort: &Ident) -> TokenStream {
    let checks = config.parts.iter().filter(|part| part.required).map(|part| {
        let field_name = part.name.to_string();
        let count_ident = part_count_ident(part);
        quote! {
            if #count_ident == 0 {
                let error = ::ras_file_core::FileError::bad_request(format!("missing required multipart field `{}`", #field_name));
                let upload_state = upload_state.take().expect("upload state is present before abort");
                service.#abort(&ctx, &path_value, upload_state, &error).await;
                return __ras_file_error_response(error);
            }
        }
    });

    quote! { #(#checks)* }
}

fn generate_download_handler(
    definition: &FileServiceDefinition,
    endpoint: &Endpoint,
    trait_name: &Ident,
) -> TokenStream {
    let handler_fn = format_ident!("{}_handler", endpoint.name);
    let handler_name = &endpoint.name;
    let path = endpoint.path.value();
    let path_struct = path_struct_name(&definition.service_name, endpoint);
    let auth = generate_auth_check(&endpoint.auth);
    let permission_check = generate_permission_check(&endpoint.auth);
    let path_extraction = generate_path_extraction(&endpoint.path_params, &path_struct);

    quote! {
        async fn #handler_fn<S, A>(
            state: ::axum::extract::State<(
                ::std::sync::Arc<S>,
                Option<::std::sync::Arc<A>>,
                Option<::std::sync::Arc<Box<dyn Fn(&::axum::http::HeaderMap, &str, &str) + Send + Sync>>>,
                Option<::std::sync::Arc<Box<dyn Fn(&str, &str, std::time::Duration) + Send + Sync>>>,
                ::ras_auth_core::AuthTransportConfig,
            )>,
            req: ::axum::http::Request<::axum::body::Body>,
        ) -> ::axum::response::Response
        where
            S: #trait_name + Send + Sync + 'static,
            A: ::ras_auth_core::AuthProvider + Send + Sync + 'static,
        {
            let start = std::time::Instant::now();
            let method = "GET";
            let request_path = req.uri().path().to_string();
            let (mut parts, _body) = req.into_parts();

            if let Some(tracker) = &state.2 {
                let tracker_headers =
                    ::ras_auth_core::redact_sensitive_headers_for_auth_transport(&parts.headers, &state.4);
                tracker(&tracker_headers, method, &request_path);
            }

            #auth
            #permission_check
            #path_extraction

            let ctx = ::ras_file_core::FileRequestContext::new(
                method,
                &request_path,
                #path,
                &parts.headers,
                user.as_ref(),
            );

            let service = &state.0.0;
            let response = match service.#handler_name(&ctx, path_value).await {
                Ok(response) => response,
                Err(error) => return __ras_file_error_response(error),
            };

            if let Some(tracker) = &state.3 {
                tracker(method, &request_path, start.elapsed());
            }

            __ras_file_download_response(response)
        }
    }
}

fn generate_path_extraction(path_params: &[PathParam], path_struct: &Ident) -> TokenStream {
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
                        return __ras_file_error_response(::ras_file_core::FileError::bad_request(format!("invalid path parameters: {}", error)));
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
                        return __ras_file_error_response(::ras_file_core::FileError::bad_request(format!("invalid path parameters: {}", error)));
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

fn generate_auth_check(auth: &AuthRequirement) -> TokenStream {
    match auth {
        AuthRequirement::Unauthorized => quote! {
            let user: Option<::ras_auth_core::AuthenticatedUser> = None;
        },
        AuthRequirement::OptionalAuth => quote! {
            // Best-effort authentication for an OPTIONAL_AUTH file route — never
            // rejected. Resolves to None for a missing/invalid credential (or a
            // cookie that fails CSRF on an unsafe method), Some(user) otherwise.
            // The caller is surfaced through FileRequestContext::new below.
            let user: Option<::ras_auth_core::AuthenticatedUser> =
                ::ras_auth_core::resolve_caller(method, &parts.headers, &state.4, state.1.as_deref())
                    .await
                    .into_authenticated();
        },
        AuthRequirement::WithPermissions(_) => quote! {
            let auth_provider = match state.1.as_ref() {
                Some(provider) => provider,
                None => return __ras_file_error_response(::ras_file_core::FileError::Internal),
            };

            let auth_credential = match ::ras_auth_core::extract_auth_credential(&parts.headers, &state.4) {
                Ok(credential) => credential,
                Err(_) => return __ras_file_error_response(::ras_file_core::FileError::Unauthorized),
            };

            if ::ras_auth_core::validate_csrf_for_credential(method, &parts.headers, &auth_credential, &state.4).is_err() {
                return __ras_file_error_response(::ras_file_core::FileError::Forbidden);
            }

            let user = match auth_provider.authenticate(auth_credential.token().to_string()).await {
                Ok(user) => Some(user),
                Err(_) => return __ras_file_error_response(::ras_file_core::FileError::Unauthorized),
            };
        },
    }
}

fn generate_permission_check(auth: &AuthRequirement) -> TokenStream {
    match auth {
        // Public routes (Unauthorized / OptionalAuth) have no permission gate.
        AuthRequirement::Unauthorized | AuthRequirement::OptionalAuth => quote! {},
        AuthRequirement::WithPermissions(permission_groups) => {
            let groups = permission_groups.iter().map(|group| {
                let perms = group.iter();
                quote! { vec![#(#perms.to_string()),*] }
            });

            quote! {
                // OR-of-AND permission check (shared ras-auth-core implementation).
                // A group list with no non-empty groups means "any authenticated
                // user", consistent with the REST and JSON-RPC macros.
                let required_permission_groups: Vec<Vec<String>> = vec![#(#groups),*];
                let authenticated_user = user.as_ref().expect("authenticated user exists after auth check");
                if ::ras_auth_core::check_permission_groups(auth_provider.as_ref(), authenticated_user, &required_permission_groups).is_err() {
                    return __ras_file_error_response(::ras_file_core::FileError::Forbidden);
                }
            }
        }
    }
}

fn generate_router_construction(endpoints: &[Endpoint], base_path: &syn::LitStr) -> TokenStream {
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

fn path_struct_name(service_name: &Ident, endpoint: &Endpoint) -> Ident {
    format_ident!(
        "{}{}Path",
        service_name,
        pascal_ident_segment(&endpoint.name.to_string())
    )
}

fn part_enum_name(service_name: &Ident, endpoint: &Endpoint) -> Ident {
    format_ident!(
        "{}{}Part",
        service_name,
        pascal_ident_segment(&endpoint.name.to_string())
    )
}

fn upload_state_type_name(endpoint: &Endpoint) -> Ident {
    format_ident!("{}State", pascal_ident_segment(&endpoint.name.to_string()))
}

pub fn part_variant_name(part: &UploadPart) -> Ident {
    format_ident!("{}", pascal_ident_segment(&part.name.to_string()))
}

fn part_count_ident(part: &UploadPart) -> Ident {
    format_ident!("{}_count", part.name)
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
