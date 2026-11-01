//! Message routing for JSON-RPC methods

use crate::{ConnectionContext, MessageHandler, ServerResult};
use async_trait::async_trait;
use ras_jsonrpc_types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, warn};

/// Function type for handling JSON-RPC method calls
pub type MethodHandler = Box<
    dyn Fn(
            JsonRpcRequest,
            Arc<ConnectionContext>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = ServerResult<Option<JsonRpcResponse>>>
                    + Send
                    + 'static,
            >,
        > + Send
        + Sync,
>;

/// Message router that dispatches JSON-RPC requests to appropriate handlers
#[derive(Default)]
pub struct MessageRouter {
    /// Map of method names to their handlers
    handlers: HashMap<String, MethodHandler>,
}

impl MessageRouter {
    /// Create a new message router
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for a specific JSON-RPC method
    pub fn register<F, Fut>(&mut self, method: &str, handler: F)
    where
        F: Fn(JsonRpcRequest, Arc<ConnectionContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<Option<JsonRpcResponse>>> + Send + 'static,
    {
        debug!("Registering handler for method: {}", method);
        let method_name = method.to_string();
        let boxed_handler = Box::new(move |req, ctx| {
            Box::pin(handler(req, ctx))
                as std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = ServerResult<Option<JsonRpcResponse>>>
                            + Send
                            + 'static,
                    >,
                >
        });
        self.handlers.insert(method_name, boxed_handler);
    }

    /// Register a handler that returns a value (automatically wrapped in JsonRpcResponse)
    pub fn register_value<F, Fut, T>(&mut self, method: &str, handler: F)
    where
        F: Fn(JsonRpcRequest, Arc<ConnectionContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<T>> + Send + 'static,
        T: serde::Serialize + Send + 'static,
    {
        debug!("Registering value handler for method: {}", method);
        let method_name = method.to_string();
        let handler = Arc::new(handler);
        let boxed_handler = Box::new(move |req: JsonRpcRequest, ctx: Arc<ConnectionContext>| {
            let handler = handler.clone();
            Box::pin(async move {
                match handler(req.clone(), ctx).await {
                    Ok(result) => {
                        // Only create response if request has an ID (not a notification)
                        if let Some(id) = req.id {
                            let response =
                                JsonRpcResponse::success(serde_json::to_value(result)?, Some(id));
                            Ok(Some(response))
                        } else {
                            Ok(None)
                        }
                    }
                    Err(e) => {
                        // Create error response if request has an ID
                        if let Some(id) = req.id {
                            let error = e
                                .public_rpc_error()
                                .unwrap_or_else(|| JsonRpcError::internal_error(e.to_string()));
                            let response = JsonRpcResponse::error(error, Some(id));
                            Ok(Some(response))
                        } else {
                            Err(e)
                        }
                    }
                }
            })
                as std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = ServerResult<Option<JsonRpcResponse>>>
                            + Send
                            + 'static,
                    >,
                >
        });
        self.handlers.insert(method_name, boxed_handler);
    }

    /// Register a notification handler (never returns a response)
    pub fn register_notification<F, Fut>(&mut self, method: &str, handler: F)
    where
        F: Fn(JsonRpcRequest, Arc<ConnectionContext>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<()>> + Send + 'static,
    {
        debug!("Registering notification handler for method: {}", method);
        let method_name = method.to_string();
        let handler = Arc::new(handler);
        let boxed_handler = Box::new(move |req: JsonRpcRequest, ctx: Arc<ConnectionContext>| {
            let handler = handler.clone();
            Box::pin(async move {
                handler(req, ctx).await?;
                Ok(None)
            })
                as std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = ServerResult<Option<JsonRpcResponse>>>
                            + Send
                            + 'static,
                    >,
                >
        });
        self.handlers.insert(method_name, boxed_handler);
    }

    /// Check if a handler is registered for a method
    pub fn has_handler(&self, method: &str) -> bool {
        self.handlers.contains_key(method)
    }

    /// Get all registered method names
    pub fn get_methods(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    /// Create error response for unknown method
    fn create_method_not_found_response(
        &self,
        request: &JsonRpcRequest,
    ) -> Option<JsonRpcResponse> {
        request.id.clone().map(|id| {
            let error = JsonRpcError::method_not_found(&request.method);
            JsonRpcResponse::error(error, Some(id))
        })
    }
}

#[async_trait]
impl MessageHandler for MessageRouter {
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        context: Arc<ConnectionContext>,
    ) -> ServerResult<Option<JsonRpcResponse>> {
        debug!("Routing request for method: {}", request.method);

        if let Some(handler) = self.handlers.get(&request.method) {
            match handler(request.clone(), context).await {
                Ok(response) => Ok(response),
                Err(e) => {
                    warn!("Handler error for method {}: {}", request.method, e);

                    // Create error response if request has an ID
                    if let Some(id) = request.id {
                        let error = e
                            .public_rpc_error()
                            .unwrap_or_else(|| JsonRpcError::internal_error(e.to_string()));
                        Ok(Some(JsonRpcResponse::error(error, Some(id))))
                    } else {
                        Err(e)
                    }
                }
            }
        } else {
            warn!("No handler found for method: {}", request.method);
            Ok(self.create_method_not_found_response(&request))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerError;
    use ras_jsonrpc_bidirectional_types::ConnectionId;
    use serde_json::json;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_router_registration() {
        let mut router = MessageRouter::new();

        // Register a simple handler
        router.register_value("test.echo", |req, _ctx| async move {
            Ok::<serde_json::Value, ServerError>(req.params.unwrap_or(json!(null)))
        });

        assert!(router.has_handler("test.echo"));
        assert!(!router.has_handler("test.unknown"));

        let methods = router.get_methods();
        assert!(methods.contains(&"test.echo".to_string()));
    }

    #[tokio::test]
    async fn test_router_handling() {
        let mut router = MessageRouter::new();

        // Register echo handler
        router.register_value("test.echo", |req, _ctx| async move {
            Ok::<serde_json::Value, ServerError>(req.params.unwrap_or(json!(null)))
        });

        // Create test context
        let connection_id = ConnectionId::new();
        let (tx, _rx) = mpsc::channel(1);
        let sender = crate::connection::ChannelMessageSender::new(connection_id, tx);
        let context = Arc::new(ConnectionContext::new(connection_id, sender));

        // Test successful request
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "test.echo".to_string(),
            params: Some(json!({"message": "hello"})),
            id: Some(json!(1)),
        };

        let response = router
            .handle_request(request, context.clone())
            .await
            .unwrap();
        assert!(response.is_some());

        let response = response.unwrap();
        assert_eq!(response.id, Some(json!(1)));
        assert!(response.result.is_some());
        assert_eq!(response.result.unwrap(), json!({"message": "hello"}));

        // Test unknown method
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "test.unknown".to_string(),
            params: None,
            id: Some(json!(2)),
        };

        let response = router.handle_request(request, context).await.unwrap();
        assert!(response.is_some());

        let response = response.unwrap();
        assert_eq!(response.id, Some(json!(2)));
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601); // METHOD_NOT_FOUND
    }

    fn test_context() -> Arc<ConnectionContext> {
        let connection_id = ConnectionId::new();
        let (tx, _rx) = mpsc::channel(1);
        let sender = crate::connection::ChannelMessageSender::new(connection_id, tx);
        Arc::new(ConnectionContext::new(connection_id, sender))
    }

    #[tokio::test]
    async fn register_low_level_handler_returns_explicit_response() {
        let mut router = MessageRouter::new();
        router.register("low.echo", |req, _ctx| async move {
            // Hand-built response — proves the low-level register path is wired.
            Ok(req
                .id
                .clone()
                .map(|id| JsonRpcResponse::success(req.params.unwrap_or(json!(null)), Some(id))))
        });

        let ctx = test_context();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "low.echo".into(),
            params: Some(json!(42)),
            id: Some(json!(7)),
        };
        let resp = router.handle_request(req, ctx).await.unwrap().unwrap();
        assert_eq!(resp.id, Some(json!(7)));
        assert_eq!(resp.result.unwrap(), json!(42));
    }

    #[tokio::test]
    async fn register_notification_never_returns_response() {
        let mut router = MessageRouter::new();
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c2 = counter.clone();
        router.register_notification("evt.tick", move |_req, _ctx| {
            let c = c2.clone();
            async move {
                c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        });

        let ctx = test_context();
        // Even if the request has an id, the notification handler returns None.
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "evt.tick".into(),
            params: None,
            id: Some(json!(1)),
        };
        assert!(router.handle_request(req, ctx).await.unwrap().is_none());
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn handler_error_with_id_becomes_error_response() {
        let mut router = MessageRouter::new();
        router.register("explode", |_req, _ctx| async move {
            Err::<Option<JsonRpcResponse>, _>(ServerError::Internal("kaboom".into()))
        });
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "explode".into(),
            params: None,
            id: Some(json!("rid")),
        };
        let resp = router.handle_request(req, test_context()).await.unwrap();
        let resp = resp.unwrap();
        assert_eq!(resp.id, Some(json!("rid")));
        // `internal_error` constructor strips details for security; the wire
        // message is the canonical "Internal error" string. We confirm the code
        // and the absence of leaked detail.
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32603);
        assert!(!err.message.contains("kaboom"));
    }

    #[tokio::test]
    async fn handler_error_without_id_propagates_as_err() {
        let mut router = MessageRouter::new();
        router.register("explode", |_req, _ctx| async move {
            Err::<Option<JsonRpcResponse>, _>(ServerError::Internal("kaboom".into()))
        });
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "explode".into(),
            params: None,
            id: None,
        };
        let result = router.handle_request(req, test_context()).await;
        assert!(matches!(result, Err(ServerError::Internal(_))));
    }

    #[tokio::test]
    async fn register_value_with_notification_request_returns_none() {
        let mut router = MessageRouter::new();
        router.register_value("v.echo", |req, _ctx| async move {
            Ok::<serde_json::Value, ServerError>(req.params.unwrap_or(json!(null)))
        });
        // No id ⇒ notification. The register_value branch should return Ok(None).
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "v.echo".into(),
            params: Some(json!(true)),
            id: None,
        };
        assert!(
            router
                .handle_request(req, test_context())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn register_value_handler_error_with_id_becomes_error_response() {
        let mut router = MessageRouter::new();
        router.register_value("v.fail", |_req, _ctx| async move {
            Err::<serde_json::Value, ServerError>(ServerError::Internal("nope".into()))
        });
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "v.fail".into(),
            params: None,
            id: Some(json!(9)),
        };
        let resp = router.handle_request(req, test_context()).await.unwrap();
        let resp = resp.unwrap();
        assert_eq!(resp.id, Some(json!(9)));
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32603);
        assert!(!err.message.contains("nope"));
    }

    fn rpc_request(method: &str) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: None,
            id: Some(json!(1)),
        }
    }

    /// Both registration APIs send a `ServerError::Rpc` as written when its
    /// code is allowed, and as `-32603` when it is reserved, so a hand-built
    /// `ServerError::Rpc` cannot bypass the handler code rule.
    #[tokio::test]
    async fn rpc_errors_follow_the_handler_code_rule_on_both_registration_apis() {
        let public = || JsonRpcError::new(1001, "Email already registered".into(), Some(json!(7)));
        let reserved = || JsonRpcError::new(-32001, "secret fake auth".into(), None);

        let mut router = MessageRouter::new();
        router.register("plain.public", move |_req, _ctx| async move {
            Err::<Option<JsonRpcResponse>, _>(ServerError::Rpc(public()))
        });
        router.register("plain.reserved", move |_req, _ctx| async move {
            Err::<Option<JsonRpcResponse>, _>(ServerError::Rpc(reserved()))
        });
        router.register_value("value.public", move |_req, _ctx| async move {
            Err::<serde_json::Value, _>(ServerError::Rpc(public()))
        });
        router.register_value("value.reserved", move |_req, _ctx| async move {
            Err::<serde_json::Value, _>(ServerError::Rpc(reserved()))
        });

        for method in ["plain.public", "value.public"] {
            let error = router
                .handle_request(rpc_request(method), test_context())
                .await
                .unwrap()
                .unwrap()
                .error
                .unwrap();
            assert_eq!(error, public(), "{method}");
        }
        for method in ["plain.reserved", "value.reserved"] {
            let error = router
                .handle_request(rpc_request(method), test_context())
                .await
                .unwrap()
                .unwrap()
                .error
                .unwrap();
            assert_eq!(error.code, -32603, "{method}");
            assert!(!error.message.contains("secret"), "{method}");
        }
    }

    #[tokio::test]
    async fn unknown_method_without_id_returns_no_response() {
        let router = MessageRouter::new();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "ghost".into(),
            params: None,
            id: None,
        };
        assert!(
            router
                .handle_request(req, test_context())
                .await
                .unwrap()
                .is_none()
        );
    }
}
