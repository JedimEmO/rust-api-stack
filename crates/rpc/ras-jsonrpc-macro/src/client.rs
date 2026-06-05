use crate::{MethodDefinition, ServiceDefinition};
use quote::quote;

/// Generate client code for JSON-RPC service
pub fn generate_client_code(service_def: &ServiceDefinition) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let client_name = quote::format_ident!("{}Client", service_name);
    let client_builder_name = quote::format_ident!("{}ClientBuilder", service_name);

    // Generate client methods
    let client_methods = service_def
        .methods
        .iter()
        .flat_map(generate_client_methods_for_method);

    let client_methods_with_timeout = service_def
        .methods
        .iter()
        .flat_map(generate_client_methods_with_timeout_for_method);

    let output = quote! {
        /// Generated client for the JSON-RPC service
        #[derive(Clone)]
        pub struct #client_name {
            transport: std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            server_url: String,
            bearer_token: Option<String>,
            default_timeout: Option<std::time::Duration>,
        }

        /// Builder for the JSON-RPC client
        pub struct #client_builder_name {
            server_url: Option<String>,
            timeout: Option<std::time::Duration>,
        }

        impl #client_builder_name {
            /// Create a new client builder
            pub fn new() -> Self {
                Self {
                    server_url: None,
                    timeout: None,
                }
            }

            /// Set the server URL
            pub fn server_url(mut self, url: impl Into<String>) -> Self {
                self.server_url = Some(url.into());
                self
            }

            /// Set the default timeout for requests
            pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            /// Build the client using the default [`ReqwestTransport`].
            pub fn build(self) -> Result<#client_name, Box<dyn std::error::Error + Send + Sync>> {
                let transport = std::sync::Arc::new(::ras_transport_core::ReqwestTransport::new());
                self.build_with_transport(transport)
            }

            /// Build the client over an explicit transport (e.g. an in-process
            /// test transport). This is the injection point used by tests.
            pub fn build_with_transport(
                self,
                transport: std::sync::Arc<dyn ::ras_transport_core::HttpTransport>,
            ) -> Result<#client_name, Box<dyn std::error::Error + Send + Sync>> {
                let server_url = self.server_url.ok_or("Server URL is required")?;

                Ok(#client_name {
                    transport,
                    server_url,
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

            #(#client_methods)*
            #(#client_methods_with_timeout)*

            /// Make a JSON-RPC request with optional timeout
            async fn make_request<T, R>(
                &self,
                method: &str,
                params: T,
                timeout: Option<std::time::Duration>,
            ) -> Result<R, ::ras_transport_core::TransportError>
            where
                T: serde::Serialize,
                R: serde::de::DeserializeOwned,
            {
                let request_body = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                    "id": 1
                });

                let mut request = ::ras_transport_core::TransportRequest::new(
                    ::ras_transport_core::http::Method::POST,
                    self.server_url.clone(),
                )
                .json(&request_body)?;

                // Add bearer token if available
                if let Some(token) = &self.bearer_token {
                    request = request.bearer(token);
                }

                // Apply per-call timeout, falling back to the client default.
                if let Some(timeout) = timeout.or(self.default_timeout) {
                    request = request.timeout(timeout);
                }

                // The transport is a dumb pipe and never inspects status. For
                // JSON-RPC, error detail lives in the body even on non-2xx
                // responses (auth/permission failures map to 401/403 but still
                // carry a JSON-RPC error envelope), so parse the body first and
                // only fall back to the HTTP status when the body is not a
                // well-formed JSON-RPC response.
                let response = self.transport.execute(request).await?;
                let status = response.status();
                let body = response.bytes().await?;

                let json_response: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(value) => value,
                    Err(err) => {
                        if status.is_success() {
                            return Err(::ras_transport_core::TransportError::Deserialize(err));
                        }
                        let text = String::from_utf8_lossy(&body).into_owned();
                        return Err(::ras_transport_core::TransportError::http_status(status, text));
                    }
                };

                // Check for JSON-RPC error
                if let Some(error) = json_response.get("error") {
                    let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| error.to_string());
                    return Err(::ras_transport_core::TransportError::JsonRpc { code, message });
                }

                // No JSON-RPC error: surface any non-success HTTP status.
                if !status.is_success() {
                    let text = String::from_utf8_lossy(&body).into_owned();
                    return Err(::ras_transport_core::TransportError::http_status(status, text));
                }

                // Extract result
                let result = json_response.get("result").ok_or_else(|| {
                    ::ras_transport_core::TransportError::Body(
                        "Missing result in JSON-RPC response".to_string(),
                    )
                })?;

                let deserialized_result: R = serde_json::from_value(result.clone())
                    .map_err(::ras_transport_core::TransportError::Deserialize)?;
                Ok(deserialized_result)
            }
        }
    };

    output
}

fn method_wire_name(method: &MethodDefinition) -> String {
    method
        .wire_name
        .clone()
        .unwrap_or_else(|| method.name.to_string())
}

fn snake_ident_segment(value: &str) -> String {
    let mut out = String::new();
    let mut pending_separator = false;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !out.is_empty() {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            pending_separator = false;
        } else {
            pending_separator = !out.is_empty();
        }
    }

    if out.is_empty() {
        "version".to_string()
    } else if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("v{out}")
    } else {
        out
    }
}

fn versioned_method_ident(method_name: &syn::Ident, version: &str) -> syn::Ident {
    let version = snake_ident_segment(version);
    quote::format_ident!("{}_{}", method_name, version)
}

/// Generate client methods for the JSON-RPC service.
fn generate_client_methods_for_method(method: &MethodDefinition) -> Vec<proc_macro2::TokenStream> {
    let mut methods = vec![generate_client_method(
        &method.name,
        method_wire_name(method),
        &method.request_type,
        &method.response_type,
    )];

    methods.extend(method.versions.iter().map(|version| {
        let method_name = versioned_method_ident(&method.name, &version.version);
        generate_client_method(
            &method_name,
            version.wire_name.clone(),
            &version.request_type,
            &version.response_type,
        )
    }));

    methods
}

fn generate_client_methods_with_timeout_for_method(
    method: &MethodDefinition,
) -> Vec<proc_macro2::TokenStream> {
    let mut methods = vec![generate_client_method_with_timeout(
        &method.name,
        method_wire_name(method),
        &method.request_type,
        &method.response_type,
    )];

    methods.extend(method.versions.iter().map(|version| {
        let method_name = versioned_method_ident(&method.name, &version.version);
        generate_client_method_with_timeout(
            &method_name,
            version.wire_name.clone(),
            &version.request_type,
            &version.response_type,
        )
    }));

    methods
}

/// Generate a client method for the JSON-RPC service
fn generate_client_method(
    method_name: &syn::Ident,
    method_str: String,
    request_type: &syn::Type,
    response_type: &syn::Type,
) -> proc_macro2::TokenStream {
    quote! {
        /// Call the #method_name method
        pub async fn #method_name(&self, params: #request_type) -> Result<#response_type, ::ras_transport_core::TransportError> {
            self.make_request(#method_str, params, None).await
        }
    }
}

/// Generate a client method with timeout for the JSON-RPC service
fn generate_client_method_with_timeout(
    method_name: &syn::Ident,
    method_str: String,
    request_type: &syn::Type,
    response_type: &syn::Type,
) -> proc_macro2::TokenStream {
    let method_name_with_timeout = quote::format_ident!("{}_with_timeout", method_name);

    quote! {
        /// Call the #method_name method with a custom timeout
        pub async fn #method_name_with_timeout(
            &self,
            params: #request_type,
            timeout: std::time::Duration
        ) -> Result<#response_type, ::ras_transport_core::TransportError> {
            self.make_request(#method_str, params, Some(timeout)).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::snake_ident_segment;

    #[test]
    fn version_labels_become_snake_case_identifier_segments() {
        assert_eq!(snake_ident_segment("v1"), "v1");
        assert_eq!(snake_ident_segment("1.0.0"), "v1_0_0");
        assert_eq!(snake_ident_segment("v1-beta"), "v1_beta");
        assert_eq!(snake_ident_segment(""), "version");
    }
}
