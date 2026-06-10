//! Server code generation for bidirectional JSON-RPC services

use crate::{AuthRequirement, BidirectionalServiceDefinition};
use quote::quote;

pub fn generate_server_code(
    service_def: &BidirectionalServiceDefinition,
) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let service_trait_name = quote::format_ident!("{}Service", service_name);
    let handler_name = quote::format_ident!("{}Handler", service_name);
    let builder_name = quote::format_ident!("{}Builder", service_name);

    // Generate a client manager trait that provides access to typed client handles
    let _client_manager_trait_name = quote::format_ident!("{}ClientManager", service_name);

    // Generate trait methods for client_to_server handlers
    let trait_methods = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;

        match &method.auth {
            AuthRequirement::Unauthorized => {
                quote! {
                    async fn #method_name(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId, connection_manager: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager, request: #request_type) -> Result<#response_type, Box<dyn std::error::Error + Send + Sync>>;
                }
            }
            AuthRequirement::WithPermissions(_) => {
                quote! {
                    async fn #method_name(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId, connection_manager: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager, user: &ras_auth_core::AuthenticatedUser, request: #request_type) -> Result<#response_type, Box<dyn std::error::Error + Send + Sync>>;
                }
            }
        }
    });

    // Generate notification methods that can be called on the service for server_to_client notifications
    let notification_methods = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let params_type = &notification.params_type;
        let method_name = quote::format_ident!("notify_{}", notification_name);

        quote! {
            async fn #method_name(&self, connection_id: ras_jsonrpc_bidirectional_types::ConnectionId, params: #params_type) -> ras_jsonrpc_bidirectional_types::Result<()>;
        }
    });

    // Generate message handler implementation
    let request_handlers = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let method_str = method_name.to_string();
        let request_type = &method.request_type;
        let permission_groups = match &method.auth {
            AuthRequirement::Unauthorized => Vec::new(),
            AuthRequirement::WithPermissions(groups) => groups.clone(),
        };

        match &method.auth {
            AuthRequirement::Unauthorized => {
                quote! {
                    #method_str => {
                        // Parse parameters
                        let params: #request_type = if let Some(params) = request.params {
                            serde_json::from_value(params)
                                .map_err(|e| ras_jsonrpc_bidirectional_server::ServerError::InvalidRequest(format!("Invalid params: {}", e)))?
                        } else {
                            // For unit type (), we can deserialize from null
                            serde_json::from_value(serde_json::Value::Null)
                                .map_err(|e| ras_jsonrpc_bidirectional_server::ServerError::InvalidRequest(format!("Invalid params: {}", e)))?
                        };

                        // Call handler with client ID and connection manager reference
                        match self.service.#method_name(context.id, self.connection_manager.as_ref(), params).await {
                            Ok(result) => {
                                let result_value = serde_json::to_value(result)
                                    .map_err(|e| ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string()))?;
                                Ok(Some(ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id.clone())))
                            }
                            Err(e) => Err(ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string())),
                        }
                    }
                }
            }
            AuthRequirement::WithPermissions(_) => {
                // Generate permission groups code for quote
                let permission_groups_code = if permission_groups.is_empty() {
                    quote! { Vec::<Vec<String>>::new() }
                } else {
                    let groups = permission_groups.iter().map(|group| {
                        let perms = group.iter();
                        quote! { vec![#(#perms.to_string()),*] }
                    });
                    quote! { vec![#(#groups),*] as Vec<Vec<String>> }
                };

                quote! {
                    #method_str => {
                        // Check if user is authenticated
                        let user = context.get_user().await
                            .ok_or_else(|| ras_jsonrpc_bidirectional_server::ServerError::AuthenticationFailed(ras_auth_core::AuthError::InvalidToken))?;

                        // OR-of-AND permission check against the cached
                        // connection user (shared ras-auth-core implementation)
                        let required_permission_groups: Vec<Vec<String>> = #permission_groups_code;
                        if !ras_auth_core::user_satisfies_permission_groups(user.as_ref(), &required_permission_groups) {
                            let error_response = ras_jsonrpc_types::JsonRpcResponse::error(
                                ras_jsonrpc_types::JsonRpcError::new(-32002, "Insufficient permissions".to_string(), None),
                                request.id.clone()
                            );
                            return Ok(Some(error_response));
                        }

                        // Parse parameters
                        let params: #request_type = if let Some(params) = request.params {
                            serde_json::from_value(params)
                                .map_err(|e| ras_jsonrpc_bidirectional_server::ServerError::InvalidRequest(format!("Invalid params: {}", e)))?
                        } else {
                            // For unit type (), we can deserialize from null
                            serde_json::from_value(serde_json::Value::Null)
                                .map_err(|e| ras_jsonrpc_bidirectional_server::ServerError::InvalidRequest(format!("Invalid params: {}", e)))?
                        };

                        // Call handler with client ID, connection manager reference, and user
                        match self.service.#method_name(context.id, self.connection_manager.as_ref(), &user, params).await {
                            Ok(result) => {
                                let result_value = serde_json::to_value(result)
                                    .map_err(|e| ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string()))?;
                                Ok(Some(ras_jsonrpc_types::JsonRpcResponse::success(result_value, request.id.clone())))
                            }
                            Err(e) => Err(ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string())),
                        }
                    }
                }
            }
        }
    });

    // Generate default notification implementations
    let default_notification_impls = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let params_type = &notification.params_type;
        let method_name = quote::format_ident!("notify_{}", notification_name);
        let notification_str = notification_name.to_string();

        quote! {
            async fn #method_name(&self, connection_id: ras_jsonrpc_bidirectional_types::ConnectionId, params: #params_type) -> ras_jsonrpc_bidirectional_types::Result<()> {
                let notification = ras_jsonrpc_bidirectional_types::ServerNotification {
                    method: #notification_str.to_string(),
                    params: serde_json::to_value(params)
                        .map_err(ras_jsonrpc_bidirectional_types::BidirectionalError::from)?,
                    metadata: None,
                };

                let message = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(notification);
                self.connection_manager.send_to_connection(connection_id, message).await
            }
        }
    });

    // Generate typed client handle for server-side management
    let client_handle_name = quote::format_ident!("{}ClientHandle", service_name);

    // Generate notification methods for client handle
    let client_handle_notification_methods = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let params_type = &notification.params_type;
        let method_name = quote::format_ident!("{}", notification_name);
        let notification_str = notification_name.to_string();

        quote! {
            /// Send a notification to this specific client
            pub async fn #method_name(&self, params: #params_type) -> ras_jsonrpc_bidirectional_types::Result<()> {
                let notification = ras_jsonrpc_bidirectional_types::ServerNotification {
                    method: #notification_str.to_string(),
                    params: serde_json::to_value(params)
                        .map_err(ras_jsonrpc_bidirectional_types::BidirectionalError::from)?,
                    metadata: None,
                };

                let message = ras_jsonrpc_bidirectional_types::BidirectionalMessage::ServerNotification(notification);
                self.connection_manager.send_to_connection(self.client_id, message).await
            }
        }
    });

    // Generate RPC call methods for client handle (server-to-client calls with return values)
    let client_handle_call_methods = service_def.server_to_client_calls.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;
        let method_str = method_name.to_string();

        quote! {
            /// Make an RPC call to this specific client and wait for response
            pub async fn #method_name(&self, params: #request_type) -> Result<#response_type, ras_jsonrpc_bidirectional_types::BidirectionalError> {
                // Generate request ID
                let request_id = serde_json::Value::String(uuid::Uuid::new_v4().to_string());

                // Create JSON-RPC request
                let request = ras_jsonrpc_types::JsonRpcRequest {
                    jsonrpc: "2.0".to_string(),
                    method: #method_str.to_string(),
                    params: Some(serde_json::to_value(params)
                        .map_err(ras_jsonrpc_bidirectional_types::BidirectionalError::from)?),
                    id: Some(request_id.clone()),
                };

                // Create pending request channel for response
                let (response_sender, response_receiver) = tokio::sync::oneshot::channel();

                // Register pending request with connection manager
                if let Err(_) = self.connection_manager.register_pending_request(self.client_id, request_id.clone(), response_sender).await {
                    return Err(ras_jsonrpc_bidirectional_types::BidirectionalError::ConnectionError("Failed to register pending request".to_string()));
                }

                // Send request
                let message = ras_jsonrpc_bidirectional_types::BidirectionalMessage::Request(request);
                if let Err(e) = self.connection_manager.send_to_connection(self.client_id, message).await {
                    // Clean up pending request on send failure
                    let _ = self.connection_manager.remove_pending_request(self.client_id, &request_id).await;
                    return Err(e);
                }

                // Wait for response with timeout
                let response = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    response_receiver
                ).await
                .map_err(|_| ras_jsonrpc_bidirectional_types::BidirectionalError::Timeout)?
                .map_err(|_| ras_jsonrpc_bidirectional_types::BidirectionalError::ConnectionError("Response channel closed".to_string()))?;

                // Check for error response
                if let Some(error) = response.error {
                    return Err(ras_jsonrpc_bidirectional_types::BidirectionalError::RpcError(error.message));
                }

                // Parse response
                let result = response.result
                    .ok_or_else(|| ras_jsonrpc_bidirectional_types::BidirectionalError::InvalidResponse("Missing result".to_string()))?;

                serde_json::from_value(result)
                    .map_err(ras_jsonrpc_bidirectional_types::BidirectionalError::from)
            }
        }
    });

    quote! {
        /// Typed client handle for server-side client management
        pub struct #client_handle_name<'a> {
            client_id: ras_jsonrpc_bidirectional_types::ConnectionId,
            connection_manager: &'a dyn ras_jsonrpc_bidirectional_types::ConnectionManager,
        }

        impl<'a> #client_handle_name<'a> {
            /// Create a new client handle
            pub fn new(client_id: ras_jsonrpc_bidirectional_types::ConnectionId, connection_manager: &'a dyn ras_jsonrpc_bidirectional_types::ConnectionManager) -> Self {
                Self { client_id, connection_manager }
            }

            /// Get the client connection ID
            pub fn client_id(&self) -> ras_jsonrpc_bidirectional_types::ConnectionId {
                self.client_id
            }

            /// Check if this client is still connected
            pub async fn is_connected(&self) -> bool {
                self.connection_manager.get_connection(self.client_id).await
                    .map(|conn| conn.is_some())
                    .unwrap_or(false)
            }

            /// Get connection information for this client
            pub async fn get_connection_info(&self) -> ras_jsonrpc_bidirectional_types::Result<Option<ras_jsonrpc_bidirectional_types::ConnectionInfo>> {
                self.connection_manager.get_connection(self.client_id).await
            }

            /// Disconnect this client
            pub async fn disconnect(&self) -> ras_jsonrpc_bidirectional_types::Result<()> {
                self.connection_manager.remove_connection(self.client_id).await
            }

            #(#client_handle_notification_methods)*
            #(#client_handle_call_methods)*
        }

        /// Generated bidirectional service trait
        #[async_trait::async_trait]
        pub trait #service_trait_name: Send + Sync + 'static {
            #(#trait_methods)*
            #(#notification_methods)*

            /// Called when a client connects
            async fn on_client_connected(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId, connection_manager: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                let _ = (client_id, connection_manager);
                Ok(())
            }

            /// Called when a client disconnects
            async fn on_client_disconnected(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId, connection_manager: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                let _ = (client_id, connection_manager);
                Ok(())
            }

            /// Called when a client authenticates
            async fn on_client_authenticated(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId, connection_manager: &dyn ras_jsonrpc_bidirectional_types::ConnectionManager, user: &ras_auth_core::AuthenticatedUser) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                let _ = (client_id, connection_manager, user);
                Ok(())
            }
        }

        /// Generated message handler for the bidirectional service
        pub struct #handler_name<T: #service_trait_name, M: ras_jsonrpc_bidirectional_types::ConnectionManager + 'static> {
            service: std::sync::Arc<T>,
            connection_manager: std::sync::Arc<M>,
        }

        impl<T: #service_trait_name, M: ras_jsonrpc_bidirectional_types::ConnectionManager + 'static> #handler_name<T, M> {
            pub fn new(
                service: std::sync::Arc<T>,
                connection_manager: std::sync::Arc<M>,
            ) -> Self {
                Self { service, connection_manager }
            }

            /// Get a typed client handle for a connection
            pub async fn get_client(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId) -> Option<#client_handle_name<'_>> {
                // Check if connection exists
                if self.connection_manager.get_connection(client_id).await
                    .map(|conn| conn.is_some())
                    .unwrap_or(false) {
                    Some(#client_handle_name::new(client_id, self.connection_manager.as_ref()))
                } else {
                    None
                }
            }

            /// Get all connected client handles
            pub async fn get_all_clients(&self) -> Result<Vec<#client_handle_name<'_>>, ras_jsonrpc_bidirectional_types::BidirectionalError> {
                let connections = self.connection_manager.get_all_connections().await?;
                Ok(connections.into_iter()
                    .map(|conn| #client_handle_name::new(conn.id, self.connection_manager.as_ref()))
                    .collect())
            }

            /// Handle client connection event
            pub async fn on_client_connected(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                self.service.on_client_connected(client_id, self.connection_manager.as_ref()).await
            }

            /// Handle client disconnection event
            pub async fn on_client_disconnected(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                self.service.on_client_disconnected(client_id, self.connection_manager.as_ref()).await
            }

            /// Handle client authentication event
            pub async fn on_client_authenticated(&self, client_id: ras_jsonrpc_bidirectional_types::ConnectionId, user: &ras_auth_core::AuthenticatedUser) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                self.service.on_client_authenticated(client_id, self.connection_manager.as_ref(), user).await
            }

            #(#default_notification_impls)*
        }

        #[async_trait::async_trait]
        impl<T: #service_trait_name, M: ras_jsonrpc_bidirectional_types::ConnectionManager + 'static> ras_jsonrpc_bidirectional_server::MessageHandler for #handler_name<T, M> {
            async fn handle_request(
                &self,
                request: ras_jsonrpc_types::JsonRpcRequest,
                context: std::sync::Arc<ras_jsonrpc_bidirectional_server::ConnectionContext>,
            ) -> ras_jsonrpc_bidirectional_server::ServerResult<Option<ras_jsonrpc_types::JsonRpcResponse>> {
                // Dispatch method
                match request.method.as_str() {
                    #(#request_handlers)*
                    _ => Err(ras_jsonrpc_bidirectional_server::ServerError::HandlerNotFound(request.method.clone()))
                }
            }

            async fn on_connect(&self, context: std::sync::Arc<ras_jsonrpc_bidirectional_server::ConnectionContext>) -> ras_jsonrpc_bidirectional_server::ServerResult<()> {
                // Call the service's on_client_connected
                if let Err(e) = self.service.on_client_connected(context.id, self.connection_manager.as_ref()).await {
                    return Err(ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string()));
                }

                // If user is authenticated, call on_client_authenticated
                if let Some(user) = context.get_user().await {
                    if let Err(e) = self.service.on_client_authenticated(context.id, self.connection_manager.as_ref(), &user).await {
                        return Err(ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string()));
                    }
                }

                Ok(())
            }

            async fn on_disconnect(&self, context: std::sync::Arc<ras_jsonrpc_bidirectional_server::ConnectionContext>, _reason: Option<String>) -> ras_jsonrpc_bidirectional_server::ServerResult<()> {
                // Call the service's on_client_disconnected
                if let Err(e) = self.service.on_client_disconnected(context.id, self.connection_manager.as_ref()).await {
                    return Err(ras_jsonrpc_bidirectional_server::ServerError::Internal(e.to_string()));
                }
                Ok(())
            }
        }

        /// Builder for the bidirectional WebSocket service
        pub struct #builder_name<T: #service_trait_name, A: ras_auth_core::AuthProvider> {
            service: std::sync::Arc<T>,
            auth_provider: std::sync::Arc<A>,
            require_auth: bool,
        }

        impl<T: #service_trait_name, A: ras_auth_core::AuthProvider> #builder_name<T, A> {
            /// Create a new builder
            ///
            /// Authentication is required by default; call `.require_auth(false)`
            /// to explicitly allow anonymous connections.
            pub fn new(service: T, auth_provider: A) -> Self {
                Self {
                    service: std::sync::Arc::new(service),
                    auth_provider: std::sync::Arc::new(auth_provider),
                    require_auth: true,
                }
            }

            /// Set whether authentication is required
            pub fn require_auth(mut self, require_auth: bool) -> Self {
                self.require_auth = require_auth;
                self
            }

            /// Build the WebSocket service
            pub fn build(self) -> ras_jsonrpc_bidirectional_server::service::BuiltWebSocketService<#handler_name<T, ras_jsonrpc_bidirectional_server::DefaultConnectionManager>, A, ras_jsonrpc_bidirectional_server::DefaultConnectionManager> {
                use ras_jsonrpc_bidirectional_server::DefaultConnectionManager;

                let connection_manager = std::sync::Arc::new(DefaultConnectionManager::new());
                let handler = #handler_name::new(
                    self.service.clone(),
                    connection_manager.clone(),
                );

                let builder = ras_jsonrpc_bidirectional_server::WebSocketServiceBuilder::builder()
                    .handler(std::sync::Arc::new(handler))
                    .auth_provider(self.auth_provider)
                    .require_auth(self.require_auth)
                    .build();
                builder.build()
            }
        }
    }
}
