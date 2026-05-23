//! Client code generation for bidirectional JSON-RPC services

use crate::BidirectionalServiceDefinition;
use quote::quote;

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars.as_str().chars()).collect(),
            }
        })
        .collect()
}

pub fn generate_client_code(
    service_def: &BidirectionalServiceDefinition,
) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let client_name = quote::format_ident!("{}Client", service_name);
    let client_builder_name = quote::format_ident!("{}ClientBuilder", service_name);
    let client_to_server_message_name =
        quote::format_ident!("{}ClientToServerMessage", service_name);
    let server_to_client_notification_name =
        quote::format_ident!("{}ServerToClientNotification", service_name);

    // Generate client method implementations for client_to_server calls
    let client_methods = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let method_str = method_name.to_string();
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        quote! {
            /// Call the #method_name method on the server
            pub async fn #method_name(&self, request: #request_type) -> ras_jsonrpc_bidirectional_client::error::ClientResult<#response_type> {
                let response = self.client.call(#method_str, Some(serde_json::to_value(request)?)).await?;
                match response.result {
                    Some(result) => {
                        Ok(serde_json::from_value(result)?)
                    }
                    None => {
                        // Check if there was an error
                        if let Some(error) = response.error {
                            Err(ras_jsonrpc_bidirectional_client::ClientError::internal(format!("JSON-RPC error: {}", error.message)))
                        } else {
                            Err(ras_jsonrpc_bidirectional_client::ClientError::internal("Response has no result or error"))
                        }
                    }
                }
            }
        }
    });

    // Generate notification handler registration methods for server_to_client notifications
    let notification_handlers = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let params_type = &notification.params_type;
        let handler_method_name = quote::format_ident!("on_{}", notification_name);
        let notification_str = notification_name.to_string();

        quote! {
            /// Register a handler for #notification_name notifications from the server
            pub fn #handler_method_name<F>(&mut self, handler: F)
            where
                F: Fn(#params_type) + Send + Sync + 'static,
            {
                let handler = std::sync::Arc::new(move |method: &str, params: &serde_json::Value| {
                    if method == #notification_str {
                        match serde_json::from_value::<#params_type>(params.clone()) {
                            Ok(typed_params) => handler(typed_params),
                            Err(e) => {
                                eprintln!("Failed to deserialize notification parameters: {}", e);
                            }
                        }
                    }
                });
                self.client.on_notification(#notification_str, handler);
            }
        }
    });

    // Generate RPC handler registration methods for server_to_client calls
    let rpc_handlers = service_def.server_to_client_calls.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;
        let handler_method_name = quote::format_ident!("on_{}", method_name);
        let method_str = method_name.to_string();

        quote! {
            /// Register a handler for #method_name RPC calls from the server
            pub fn #handler_method_name<F, Fut>(&mut self, handler: F)
            where
                F: Fn(#request_type) -> Fut + Send + Sync + 'static,
                Fut: std::future::Future<Output = Result<#response_type, String>> + Send + 'static,
            {
                let callback = std::sync::Arc::new(handler);
                let handler = std::sync::Arc::new(move |request: ras_jsonrpc_types::JsonRpcRequest| {
                    let callback = callback.clone();
                    Box::pin(async move {
                        // Parse request parameters
                        let params: #request_type = if let Some(params) = request.params {
                            match serde_json::from_value(params) {
                                Ok(p) => p,
                                Err(e) => {
                                    return ras_jsonrpc_types::JsonRpcResponse::error(
                                        ras_jsonrpc_types::JsonRpcError::new(-32602, format!("Invalid params: {}", e), None),
                                        request.id
                                    );
                                }
                            }
                        } else {
                            match serde_json::from_value(serde_json::Value::Null) {
                                Ok(p) => p,
                                Err(e) => {
                                    return ras_jsonrpc_types::JsonRpcResponse::error(
                                        ras_jsonrpc_types::JsonRpcError::new(-32602, format!("Invalid params: {}", e), None),
                                        request.id
                                    );
                                }
                            }
                        };

                        // Call handler
                        match callback(params).await {
                            Ok(result) => {
                                match serde_json::to_value(result) {
                                    Ok(result_value) => ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id),
                                    Err(e) => ras_jsonrpc_types::JsonRpcResponse::error(
                                        ras_jsonrpc_types::JsonRpcError::new(-32603, format!("Failed to serialize result: {}", e), None),
                                        request.id
                                    ),
                                }
                            }
                            Err(error_msg) => {
                                ras_jsonrpc_types::JsonRpcResponse::error(
                                    ras_jsonrpc_types::JsonRpcError::new(-32000, error_msg, None),
                                    request.id
                                )
                            }
                        }
                    }) as std::pin::Pin<Box<dyn std::future::Future<Output = ras_jsonrpc_types::JsonRpcResponse> + Send>>
                });
                self.client.on_rpc_request(#method_str, handler);
            }
        }
    });

    // Generate client message enum types
    let client_to_server_methods = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let variant_name = quote::format_ident!("{}", to_pascal_case(&method_name.to_string()));
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        quote! {
            #variant_name {
                request: #request_type,
                response_sender: tokio::sync::oneshot::Sender<Result<#response_type, ras_jsonrpc_bidirectional_client::ClientError>>,
            },
        }
    });

    let server_to_client_notifications = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let variant_name =
            quote::format_ident!("{}", to_pascal_case(&notification_name.to_string()));
        let params_type = &notification.params_type;

        quote! {
            #variant_name(#params_type),
        }
    });

    quote! {
        #[cfg(feature = "client")]
        /// Generated client for the bidirectional service
        pub struct #client_name {
            client: ras_jsonrpc_bidirectional_client::Client,
        }

        #[cfg(feature = "client")]
        impl #client_name {
            /// Create a new client from a pre-configured Client
            pub fn new(client: ras_jsonrpc_bidirectional_client::Client) -> Self {
                Self { client }
            }

            /// Get the underlying client
            pub fn client(&self) -> &ras_jsonrpc_bidirectional_client::Client {
                &self.client
            }

            /// Get a mutable reference to the underlying client
            pub fn client_mut(&mut self) -> &mut ras_jsonrpc_bidirectional_client::Client {
                &mut self.client
            }

            #(#client_methods)*

            #(#notification_handlers)*

            #(#rpc_handlers)*

            /// Connect to the WebSocket server
            pub async fn connect(&self) -> ras_jsonrpc_bidirectional_client::error::ClientResult<()> {
                self.client.connect().await
            }

            /// Disconnect from the WebSocket server
            pub async fn disconnect(&self) -> ras_jsonrpc_bidirectional_client::error::ClientResult<()> {
                self.client.disconnect().await
            }

            /// Check if the client is connected
            pub async fn is_connected(&self) -> bool {
                self.client.is_connected().await
            }

            /// Subscribe to a topic for broadcast messages
            pub async fn subscribe(&self, topic: &str, handler: ras_jsonrpc_bidirectional_client::NotificationHandler) -> ras_jsonrpc_bidirectional_client::error::ClientResult<()> {
                self.client.subscribe(topic, handler).await
            }

            /// Unsubscribe from a topic
            pub async fn unsubscribe(&self, topic: &str) -> ras_jsonrpc_bidirectional_client::error::ClientResult<()> {
                self.client.unsubscribe(topic).await
            }
        }

        #[cfg(feature = "client")]
        /// Builder for the bidirectional client
        pub struct #client_builder_name {
            url: String,
            jwt_token: Option<String>,
            timeout: Option<std::time::Duration>,
        }

        #[cfg(feature = "client")]
        impl #client_builder_name {
            /// Create a new client builder
            pub fn new(url: impl Into<String>) -> Self {
                Self {
                    url: url.into(),
                    jwt_token: None,
                    timeout: None,
                }
            }

            /// Set JWT token for authentication
            pub fn with_jwt_token(mut self, token: String) -> Self {
                self.jwt_token = Some(token);
                self
            }

            /// Set request timeout
            pub fn with_request_timeout(mut self, timeout: std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            /// Build the client
            pub async fn build(self) -> ras_jsonrpc_bidirectional_client::error::ClientResult<#client_name> {
                let mut builder = ras_jsonrpc_bidirectional_client::ClientBuilder::new(&self.url);

                if let Some(token) = self.jwt_token {
                    builder = builder.with_jwt_token(token);
                }

                if let Some(timeout) = self.timeout {
                    builder = builder.with_request_timeout(timeout);
                }

                let client = builder.build().await?;
                Ok(#client_name::new(client))
            }
        }

        #[cfg(feature = "client")]
        /// Type-safe enum for client-to-server messages
        #[derive(Debug)]
        pub enum #client_to_server_message_name {
            #(#client_to_server_methods)*
        }

        #[cfg(feature = "client")]
        /// Type-safe enum for server-to-client notifications
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub enum #server_to_client_notification_name {
            #(#server_to_client_notifications)*
        }
    }
}
