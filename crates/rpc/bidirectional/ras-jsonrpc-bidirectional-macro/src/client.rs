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

    let client_crate = quote! { ::ras_jsonrpc_bidirectional_client };
    let private = quote! { #client_crate::__private };
    let serde_path = "::ras_jsonrpc_bidirectional_client::__private::serde";

    // One typed method per client_to_server call.
    let client_methods = service_def.client_to_server.iter().map(|method| {
        let method_name = &method.name;
        let method_str = method_name.to_string();
        let request_type = &method.request_type;
        let response_type = &method.response_type;
        let doc = format!("Call `{method_str}` on the server.");

        quote! {
            #[doc = #doc]
            pub async fn #method_name(&self, request: #request_type) -> #client_crate::error::ClientResult<#response_type> {
                #private::call_method(&self.client, #method_str, request).await
            }
        }
    });

    // One `on_<name>` registration per server_to_client notification.
    let notification_handlers = service_def.server_to_client.iter().map(|notification| {
        let notification_name = &notification.name;
        let params_type = &notification.params_type;
        let handler_method_name = quote::format_ident!("on_{}", notification_name);
        let notification_str = notification_name.to_string();
        let doc = format!(
            "Handle the `{notification_str}` notification. Params that do not decode are logged \
             and dropped."
        );

        quote! {
            #[doc = #doc]
            pub fn #handler_method_name<F>(&mut self, handler: F)
            where
                F: Fn(#params_type) + Send + Sync + 'static,
            {
                self.client.on_notification(
                    #notification_str,
                    #private::notification_handler::<#params_type, _>(#notification_str, handler),
                );
            }
        }
    });

    // One `on_<name>` registration per server_to_client call.
    let rpc_handlers = service_def.server_to_client_calls.iter().map(|method| {
        let method_name = &method.name;
        let request_type = &method.request_type;
        let response_type = &method.response_type;
        let handler_method_name = quote::format_ident!("on_{}", method_name);
        let method_str = method_name.to_string();
        let doc = format!(
            "Answer the server's `{method_str}` call. An `Err` reaches the server as code \
             `-32000` with the string as its message."
        );

        quote! {
            #[doc = #doc]
            pub fn #handler_method_name<F, Fut>(&mut self, handler: F)
            where
                F: Fn(#request_type) -> Fut + Send + Sync + 'static,
                Fut: ::std::future::Future<Output = ::std::result::Result<#response_type, ::std::string::String>> + Send + 'static,
            {
                self.client.on_rpc_request(
                    #method_str,
                    #private::rpc_handler::<#request_type, #response_type, _, _>(handler),
                );
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
                response_sender: #private::tokio::sync::oneshot::Sender<::std::result::Result<#response_type, #client_crate::ClientError>>,
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
        /// Generated client for the bidirectional service
        pub struct #client_name {
            client: #client_crate::Client,
        }

        impl #client_name {
            /// Create a new client from a pre-configured Client
            pub fn new(client: #client_crate::Client) -> Self {
                Self { client }
            }

            /// Get the underlying client
            pub fn client(&self) -> &#client_crate::Client {
                &self.client
            }

            /// Get a mutable reference to the underlying client
            pub fn client_mut(&mut self) -> &mut #client_crate::Client {
                &mut self.client
            }

            #(#client_methods)*

            #(#notification_handlers)*

            #(#rpc_handlers)*

            /// Connect to the WebSocket server
            pub async fn connect(&self) -> #client_crate::error::ClientResult<()> {
                self.client.connect().await
            }

            /// Disconnect from the WebSocket server
            pub async fn disconnect(&self) -> #client_crate::error::ClientResult<()> {
                self.client.disconnect().await
            }

            /// Check if the client is connected
            pub async fn is_connected(&self) -> bool {
                self.client.is_connected().await
            }

            /// Subscribe to a topic for broadcast messages
            pub async fn subscribe(&self, topic: &str, handler: #client_crate::NotificationHandler) -> #client_crate::error::ClientResult<()> {
                self.client.subscribe(topic, handler).await
            }

            /// Unsubscribe from a topic
            pub async fn unsubscribe(&self, topic: &str) -> #client_crate::error::ClientResult<()> {
                self.client.unsubscribe(topic).await
            }
        }

        /// Builder for the bidirectional client
        pub struct #client_builder_name {
            url: ::std::string::String,
            jwt_token: ::std::option::Option<::std::string::String>,
            timeout: ::std::option::Option<::std::time::Duration>,
        }

        impl #client_builder_name {
            /// Create a new client builder
            pub fn new(url: impl Into<::std::string::String>) -> Self {
                Self {
                    url: url.into(),
                    jwt_token: None,
                    timeout: None,
                }
            }

            /// Set JWT token for authentication
            pub fn with_jwt_token(mut self, token: ::std::string::String) -> Self {
                self.jwt_token = Some(token);
                self
            }

            /// Set request timeout
            pub fn with_request_timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.timeout = Some(timeout);
                self
            }

            /// Build the client
            pub async fn build(self) -> #client_crate::error::ClientResult<#client_name> {
                let mut builder = #client_crate::ClientBuilder::new(&self.url);

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

        /// Type-safe enum for client-to-server messages
        #[derive(Debug)]
        pub enum #client_to_server_message_name {
            #(#client_to_server_methods)*
        }

        /// Type-safe enum for server-to-client notifications
        #[derive(Debug, Clone, #private::serde::Serialize, #private::serde::Deserialize)]
        #[serde(crate = #serde_path)]
        pub enum #server_to_client_notification_name {
            #(#server_to_client_notifications)*
        }
    }
}
