//! Server code generation for bidirectional JSON-RPC services
//!
//! Every path in the generated code goes through `ras_jsonrpc_bidirectional_server`, mostly its
//! hidden `__private` module, so the API crate needs no other runtime dependency.

use crate::{AuthRequirement, BidirectionalServiceDefinition};
use quote::quote;

pub fn generate_server_code(
    service_def: &BidirectionalServiceDefinition,
) -> proc_macro2::TokenStream {
    let service_name = &service_def.service_name;
    let service_trait_name = quote::format_ident!("{}Service", service_name);
    let handler_name = quote::format_ident!("{}Handler", service_name);
    let builder_name = quote::format_ident!("{}Builder", service_name);
    let client_handle_name = quote::format_ident!("{}ClientHandle", service_name);

    let server = quote! { ::ras_jsonrpc_bidirectional_server };
    let private = quote! { #server::__private };
    let types = quote! { #private::ras_jsonrpc_bidirectional_types };
    let auth = quote! { #private::ras_auth_core };
    let jsonrpc = quote! { #private::ras_jsonrpc_types };
    let async_trait = quote! { #private::async_trait::async_trait };
    let handler_doc = format!(
        "Dispatches requests to a [`{service_trait_name}`] implementation and checks each \
         method's auth requirement."
    );
    let builder_doc =
        format!("Builds the WebSocket service for a [`{service_trait_name}`] implementation.");
    let handler_error = quote! { ::std::boxed::Box<dyn ::std::error::Error + Send + Sync> };

    // One trait method per client_to_server method.
    let trait_methods = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;
        let caller_param = match &method.auth {
            AuthRequirement::Unauthorized => quote! {},
            AuthRequirement::OptionalAuth => quote! { caller: #auth::Caller, },
            AuthRequirement::WithPermissions(_) => quote! { user: &#auth::AuthenticatedUser, },
        };

        quote! {
            async fn #method_name(
                &self,
                client_id: #types::ConnectionId,
                connection_manager: &dyn #types::ConnectionManager,
                #caller_param
                request: #request_type,
            ) -> ::std::result::Result<#response_type, #handler_error>;
        }
    });

    // Dispatch arms of the generated MessageHandler.
    let request_handlers = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let method_str = method_name.to_string();
        let request_type = &method.request_type;

        let (auth_check, caller_arg) = match &method.auth {
            AuthRequirement::Unauthorized => (quote! {}, quote! {}),
            AuthRequirement::OptionalAuth => (
                quote! {
                    // OPTIONAL_AUTH: never rejected; the handler gets the
                    // connection's user, if any, as a Caller.
                    let caller = #auth::Caller::from(
                        context.get_user().await.map(|user| (*user).clone()),
                    );
                },
                quote! { caller, },
            ),
            AuthRequirement::WithPermissions(groups) => {
                let groups = groups.iter().map(|group| {
                    quote! { ::std::vec![#(::std::string::String::from(#group)),*] }
                });
                (
                    quote! {
                        let user = context.get_user().await.ok_or_else(|| {
                            #server::ServerError::AuthenticationFailed(#auth::AuthError::InvalidToken)
                        })?;

                        // OR-of-AND check against the cached connection user,
                        // through the auth provider when one is attached.
                        let required_permission_groups: ::std::vec::Vec<::std::vec::Vec<::std::string::String>> =
                            ::std::vec![#(#groups),*];
                        if !self.permission_check_passes(user.as_ref(), &required_permission_groups) {
                            let required = required_permission_groups
                                .iter()
                                .find(|group| !group.is_empty())
                                .cloned()
                                .unwrap_or_default();
                            return Ok(Some(#jsonrpc::JsonRpcResponse::error(
                                #jsonrpc::JsonRpcError::insufficient_permissions(required),
                                request.id.clone(),
                            )));
                        }
                    },
                    quote! { &user, },
                )
            }
        };

        quote! {
            #method_str => {
                #auth_check
                let id = request.id;
                let params: #request_type = #private::decode_params(request.params)?;
                let result = self
                    .service
                    .#method_name(context.id, self.connection_manager.as_ref(), #caller_arg params)
                    .await;
                #private::handler_response(id, result)
            }
        }
    });

    // Typed notification senders on the handler: `notify_<name>(connection_id, params)`.
    let handler_notification_methods = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let params_type = &notification.params_type;
        let method_name = quote::format_ident!("notify_{}", notification_name);
        let notification_str = notification_name.to_string();

        quote! {
            /// Send this notification to one connection.
            pub async fn #method_name(
                &self,
                connection_id: #types::ConnectionId,
                params: #params_type,
            ) -> #types::Result<()> {
                #private::send_notification(
                    self.connection_manager.as_ref(),
                    connection_id,
                    #notification_str,
                    params,
                )
                .await
            }
        }
    });

    // Typed notification senders on the client handle.
    let client_handle_notification_methods =
        service_def.server_to_client.iter().map(|notification| {
            let notification_name = &notification.name;
            let params_type = &notification.params_type;
            let notification_str = notification_name.to_string();

            quote! {
                /// Send this notification to the client.
                pub async fn #notification_name(&self, params: #params_type) -> #types::Result<()> {
                    #private::send_notification(
                        self.connection_manager,
                        self.client_id,
                        #notification_str,
                        params,
                    )
                    .await
                }
            }
        });

    // Server-to-client calls on the client handle.
    let client_handle_call_methods = service_def.server_to_client_calls.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;
        let method_str = method_name.to_string();

        quote! {
            /// Call this method on the client and wait for its answer, up to the handle's
            /// call timeout.
            pub async fn #method_name(
                &self,
                params: #request_type,
            ) -> ::std::result::Result<#response_type, #types::BidirectionalError> {
                #private::call_client(
                    self.connection_manager,
                    self.client_id,
                    #method_str,
                    params,
                    self.call_timeout,
                )
                .await
            }
        }
    });

    quote! {
        /// Typed handle to one connected client: send it notifications and call its methods.
        pub struct #client_handle_name<'a> {
            client_id: #types::ConnectionId,
            connection_manager: &'a dyn #types::ConnectionManager,
            call_timeout: ::std::time::Duration,
        }

        impl<'a> #client_handle_name<'a> {
            /// Create a handle for `client_id`, with the default call timeout
            /// (`DEFAULT_CLIENT_CALL_TIMEOUT`, 30 seconds).
            pub fn new(
                client_id: #types::ConnectionId,
                connection_manager: &'a dyn #types::ConnectionManager,
            ) -> Self {
                Self {
                    client_id,
                    connection_manager,
                    call_timeout: #server::service::DEFAULT_CLIENT_CALL_TIMEOUT,
                }
            }

            /// How long a server-to-client call waits for the client's answer before it
            /// fails with `BidirectionalError::Timeout`.
            pub fn with_call_timeout(mut self, call_timeout: ::std::time::Duration) -> Self {
                self.call_timeout = call_timeout;
                self
            }

            /// The timeout for server-to-client calls made through this handle.
            pub fn call_timeout(&self) -> ::std::time::Duration {
                self.call_timeout
            }

            /// The client's connection ID.
            pub fn client_id(&self) -> #types::ConnectionId {
                self.client_id
            }

            /// Whether the client is still connected.
            pub async fn is_connected(&self) -> bool {
                self.connection_manager
                    .get_connection(self.client_id)
                    .await
                    .map(|conn| conn.is_some())
                    .unwrap_or(false)
            }

            /// Connection information for the client.
            pub async fn get_connection_info(&self) -> #types::Result<::std::option::Option<#types::ConnectionInfo>> {
                self.connection_manager.get_connection(self.client_id).await
            }

            /// Disconnect the client: the connection stops taking requests, sends what is
            /// already queued for it and closes.
            pub async fn disconnect(&self) -> #types::Result<()> {
                self.connection_manager.remove_connection(self.client_id).await
            }

            #(#client_handle_notification_methods)*
            #(#client_handle_call_methods)*
        }

        /// The service to implement: one method per `client_to_server` entry, plus connection
        /// callbacks and the topic subscription policy.
        #[#async_trait]
        pub trait #service_trait_name: Send + Sync + 'static {
            #(#trait_methods)*

            /// Called when a client connects
            async fn on_client_connected(
                &self,
                client_id: #types::ConnectionId,
                connection_manager: &dyn #types::ConnectionManager,
            ) -> ::std::result::Result<(), #handler_error> {
                let _ = (client_id, connection_manager);
                Ok(())
            }

            /// Called when a client disconnects
            async fn on_client_disconnected(
                &self,
                client_id: #types::ConnectionId,
                connection_manager: &dyn #types::ConnectionManager,
            ) -> ::std::result::Result<(), #handler_error> {
                let _ = (client_id, connection_manager);
                Ok(())
            }

            /// Called when a client connects with a valid credential
            async fn on_client_authenticated(
                &self,
                client_id: #types::ConnectionId,
                connection_manager: &dyn #types::ConnectionManager,
                user: &#auth::AuthenticatedUser,
            ) -> ::std::result::Result<(), #handler_error> {
                let _ = (client_id, connection_manager, user);
                Ok(())
            }

            /// Decide whether the connection may subscribe to `topic`.
            ///
            /// Default-deny. `user` is the connection's current user, or `None` for an
            /// anonymous connection. The decision is asked again for every held topic when the
            /// connection's credential is re-validated, so a revoked permission drops the
            /// subscription. An `Err` closes the connection.
            async fn authorize_subscribe(
                &self,
                client_id: #types::ConnectionId,
                connection_manager: &dyn #types::ConnectionManager,
                user: ::std::option::Option<&#auth::AuthenticatedUser>,
                topic: &str,
            ) -> ::std::result::Result<bool, #handler_error> {
                let _ = (client_id, connection_manager, user, topic);
                Ok(false)
            }
        }

        #[doc = #handler_doc]
        pub struct #handler_name<T: #service_trait_name, M: #types::ConnectionManager + 'static> {
            service: ::std::sync::Arc<T>,
            connection_manager: ::std::sync::Arc<M>,
            /// Provider whose `check_permissions` decides WITH_PERMISSIONS
            /// methods. Without one the handler falls back to plain
            /// set-membership against the cached connection user.
            auth_provider: ::std::option::Option<::std::sync::Arc<dyn #auth::AuthProvider>>,
            client_call_timeout: ::std::time::Duration,
        }

        impl<T: #service_trait_name, M: #types::ConnectionManager + 'static> #handler_name<T, M> {
            pub fn new(service: ::std::sync::Arc<T>, connection_manager: ::std::sync::Arc<M>) -> Self {
                Self {
                    service,
                    connection_manager,
                    auth_provider: None,
                    client_call_timeout: #server::service::DEFAULT_CLIENT_CALL_TIMEOUT,
                }
            }

            /// Route WITH_PERMISSIONS checks through this provider's
            /// `check_permissions`, so custom permission semantics (wildcards,
            /// hierarchies, dynamic denies) apply over WebSocket exactly as
            /// they do over REST and JSON-RPC. The generated builder sets this
            /// automatically; call it when constructing the handler by hand.
            pub fn with_auth_provider(mut self, auth_provider: ::std::sync::Arc<dyn #auth::AuthProvider>) -> Self {
                self.auth_provider = Some(auth_provider);
                self
            }

            /// Call timeout for the client handles returned by `get_client` and
            /// `get_all_clients`. Defaults to `DEFAULT_CLIENT_CALL_TIMEOUT` (30 seconds).
            pub fn with_client_call_timeout(mut self, call_timeout: ::std::time::Duration) -> Self {
                self.client_call_timeout = call_timeout;
                self
            }

            /// The connection manager this handler sends through.
            pub fn connection_manager(&self) -> &::std::sync::Arc<M> {
                &self.connection_manager
            }

            fn permission_check_passes(
                &self,
                user: &#auth::AuthenticatedUser,
                groups: &[::std::vec::Vec<::std::string::String>],
            ) -> bool {
                match &self.auth_provider {
                    Some(provider) => #auth::check_permission_groups(provider.as_ref(), user, groups).is_ok(),
                    None => #auth::user_satisfies_permission_groups(user, groups),
                }
            }

            /// A typed handle for a connected client, or `None` when it is not connected.
            pub async fn get_client(&self, client_id: #types::ConnectionId) -> ::std::option::Option<#client_handle_name<'_>> {
                let connected = self
                    .connection_manager
                    .get_connection(client_id)
                    .await
                    .map(|conn| conn.is_some())
                    .unwrap_or(false);
                connected.then(|| {
                    #client_handle_name::new(client_id, self.connection_manager.as_ref())
                        .with_call_timeout(self.client_call_timeout)
                })
            }

            /// Typed handles for every connected client.
            pub async fn get_all_clients(&self) -> ::std::result::Result<::std::vec::Vec<#client_handle_name<'_>>, #types::BidirectionalError> {
                let connections = self.connection_manager.get_all_connections().await?;
                Ok(connections
                    .into_iter()
                    .map(|conn| {
                        #client_handle_name::new(conn.id, self.connection_manager.as_ref())
                            .with_call_timeout(self.client_call_timeout)
                    })
                    .collect())
            }

            /// Handle client connection event
            pub async fn on_client_connected(&self, client_id: #types::ConnectionId) -> ::std::result::Result<(), #handler_error> {
                self.service.on_client_connected(client_id, self.connection_manager.as_ref()).await
            }

            /// Handle client disconnection event
            pub async fn on_client_disconnected(&self, client_id: #types::ConnectionId) -> ::std::result::Result<(), #handler_error> {
                self.service.on_client_disconnected(client_id, self.connection_manager.as_ref()).await
            }

            /// Handle client authentication event
            pub async fn on_client_authenticated(&self, client_id: #types::ConnectionId, user: &#auth::AuthenticatedUser) -> ::std::result::Result<(), #handler_error> {
                self.service.on_client_authenticated(client_id, self.connection_manager.as_ref(), user).await
            }

            #(#handler_notification_methods)*
        }

        #[#async_trait]
        impl<T: #service_trait_name, M: #types::ConnectionManager + 'static> #server::MessageHandler for #handler_name<T, M> {
            async fn handle_request(
                &self,
                request: #jsonrpc::JsonRpcRequest,
                context: ::std::sync::Arc<#server::ConnectionContext>,
            ) -> #server::ServerResult<::std::option::Option<#jsonrpc::JsonRpcResponse>> {
                match request.method.as_str() {
                    #(#request_handlers)*
                    _ => Err(#server::ServerError::HandlerNotFound(request.method.clone())),
                }
            }

            async fn authorize_subscribe(
                &self,
                topic: &str,
                context: &::std::sync::Arc<#server::ConnectionContext>,
            ) -> #server::ServerResult<bool> {
                let user = context.get_user().await;
                self.service
                    .authorize_subscribe(
                        context.id,
                        self.connection_manager.as_ref(),
                        user.as_deref(),
                        topic,
                    )
                    .await
                    .map_err(|e| #server::ServerError::Internal(e.to_string()))
            }

            async fn on_connect(&self, context: ::std::sync::Arc<#server::ConnectionContext>) -> #server::ServerResult<()> {
                if let Err(e) = self.service.on_client_connected(context.id, self.connection_manager.as_ref()).await {
                    return Err(#server::ServerError::Internal(e.to_string()));
                }

                if let Some(user) = context.get_user().await {
                    if let Err(e) = self.service.on_client_authenticated(context.id, self.connection_manager.as_ref(), &user).await {
                        return Err(#server::ServerError::Internal(e.to_string()));
                    }
                }

                Ok(())
            }

            async fn on_disconnect(&self, context: ::std::sync::Arc<#server::ConnectionContext>, _reason: ::std::option::Option<::std::string::String>) -> #server::ServerResult<()> {
                if let Err(e) = self.service.on_client_disconnected(context.id, self.connection_manager.as_ref()).await {
                    return Err(#server::ServerError::Internal(e.to_string()));
                }
                Ok(())
            }
        }

        #[doc = #builder_doc]
        pub struct #builder_name<T: #service_trait_name, A: #auth::AuthProvider> {
            service: ::std::sync::Arc<T>,
            auth_provider: ::std::sync::Arc<A>,
            require_auth: bool,
            max_connections: ::std::option::Option<usize>,
            max_message_size: ::std::option::Option<usize>,
            message_channel_capacity: ::std::option::Option<usize>,
            subscription_limits: ::std::option::Option<#server::SubscriptionLimits>,
            keepalive: ::std::option::Option<#server::KeepaliveConfig>,
            on_permission_change: ::std::option::Option<#server::PermissionChangePolicy>,
            auth_revalidation_interval: ::std::option::Option<::std::time::Duration>,
            client_call_timeout: ::std::time::Duration,
        }

        impl<T: #service_trait_name, A: #auth::AuthProvider> #builder_name<T, A> {
            /// Create a new builder
            ///
            /// Authentication is required by default; call `.require_auth(false)`
            /// to explicitly allow anonymous connections.
            pub fn new(service: T, auth_provider: A) -> Self {
                Self {
                    service: ::std::sync::Arc::new(service),
                    auth_provider: ::std::sync::Arc::new(auth_provider),
                    require_auth: true,
                    max_connections: Some(#server::service::DEFAULT_MAX_CONNECTIONS),
                    max_message_size: None,
                    message_channel_capacity: None,
                    subscription_limits: None,
                    keepalive: None,
                    on_permission_change: None,
                    auth_revalidation_interval: None,
                    client_call_timeout: #server::service::DEFAULT_CLIENT_CALL_TIMEOUT,
                }
            }

            /// Set whether authentication is required
            pub fn require_auth(mut self, require_auth: bool) -> Self {
                self.require_auth = require_auth;
                self
            }

            /// Cap on simultaneous connections (`None` lifts the cap). Defaults
            /// to `DEFAULT_MAX_CONNECTIONS`; admission is atomic.
            pub fn max_connections(mut self, max_connections: ::std::option::Option<usize>) -> Self {
                self.max_connections = max_connections;
                self
            }

            /// Maximum inbound message size in bytes, enforced at the transport.
            pub fn max_message_size(mut self, max_message_size: usize) -> Self {
                self.max_message_size = Some(max_message_size);
                self
            }

            /// Messages queued for one connection's socket before the connection counts as too
            /// slow and is closed. Defaults to 1024.
            pub fn message_channel_capacity(mut self, capacity: usize) -> Self {
                self.message_channel_capacity = Some(capacity);
                self
            }

            /// Per-message, per-connection, topic-length and global subscription caps.
            pub fn subscription_limits(mut self, limits: #server::SubscriptionLimits) -> Self {
                self.subscription_limits = Some(limits);
                self
            }

            /// Server ping interval and idle timeout.
            pub fn keepalive(mut self, keepalive: #server::KeepaliveConfig) -> Self {
                self.keepalive = Some(keepalive);
                self
            }

            /// What to do when a live connection's permissions change on re-validation.
            pub fn on_permission_change(mut self, policy: #server::PermissionChangePolicy) -> Self {
                self.on_permission_change = Some(policy);
                self
            }

            /// How often a live connection's token is re-run through the auth provider.
            /// Defaults to 30 seconds.
            pub fn auth_revalidation_interval(mut self, interval: ::std::time::Duration) -> Self {
                self.auth_revalidation_interval = Some(interval);
                self
            }

            /// Call timeout for the client handles the handler hands out from `get_client`
            /// and `get_all_clients`. Defaults to `DEFAULT_CLIENT_CALL_TIMEOUT` (30 seconds).
            pub fn client_call_timeout(mut self, call_timeout: ::std::time::Duration) -> Self {
                self.client_call_timeout = call_timeout;
                self
            }

            /// Build the service with a new `DefaultConnectionManager`.
            pub fn build(self) -> #server::service::BuiltWebSocketService<#handler_name<T, #server::DefaultConnectionManager>, A, #server::DefaultConnectionManager> {
                let manager = ::std::sync::Arc::new(
                    #server::DefaultConnectionManager::with_subscription_limits(
                        self.subscription_limits.unwrap_or_default(),
                    ),
                );
                self.build_with_manager(manager)
            }

            /// Build the service around an existing connection manager, so other parts of the
            /// server can send and broadcast through the same one.
            pub fn build_with_manager<M: #types::ConnectionManager + 'static>(
                self,
                manager: ::std::sync::Arc<M>,
            ) -> #server::service::BuiltWebSocketService<#handler_name<T, M>, A, M> {
                let auth_provider_dyn: ::std::sync::Arc<dyn #auth::AuthProvider> = self.auth_provider.clone();
                let handler = #handler_name::new(self.service, manager.clone())
                    .with_auth_provider(auth_provider_dyn)
                    .with_client_call_timeout(self.client_call_timeout);

                #server::WebSocketServiceBuilder::builder()
                    .handler(::std::sync::Arc::new(handler))
                    .auth_provider(self.auth_provider)
                    .require_auth(self.require_auth)
                    .max_connections(self.max_connections)
                    .maybe_max_message_size(self.max_message_size)
                    .maybe_message_channel_capacity(self.message_channel_capacity)
                    .subscription_limits(self.subscription_limits.unwrap_or_default())
                    .maybe_keepalive(self.keepalive)
                    .maybe_on_permission_change(self.on_permission_change)
                    .maybe_auth_revalidation_interval(self.auth_revalidation_interval)
                    .build()
                    .build_with_manager(manager)
            }
        }
    }
}
