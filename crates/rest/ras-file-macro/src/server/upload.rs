use super::auth::{generate_auth_check, generate_permission_check};
use super::routes::generate_path_extraction;
use super::types::{part_enum_name, part_variant_name, path_struct_name};
use crate::parser::{
    Endpoint, FileServiceDefinition, FilenamePolicy, MaxBytes, UploadConfig, UploadPart,
    UploadPartKind,
};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

pub(super) fn generate_upload_handler(
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
                Err(rejection) => {
                    // Never echo the axum rejection body (it can include the
                    // offending header value); log it and send a fixed message.
                    ::ras_file_core::tracing::warn!(
                        status = rejection.status().as_u16(),
                        detail = %::ras_file_core::sanitize_log_detail(&rejection.body_text()),
                        "rejected request: invalid multipart request"
                    );
                    return __ras_file_error_response(
                        ::ras_file_core::FileError::bad_request("invalid multipart request"),
                    );
                }
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
                    // Reduce the client-supplied name to a single safe path
                    // component before the handler ever sees it.
                    let file_name = field.file_name().map(::ras_file_core::sanitize_filename);
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

fn part_count_ident(part: &UploadPart) -> Ident {
    format_ident!("{}_count", part.name)
}
