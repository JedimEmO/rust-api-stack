use super::Client;
use crate::{
    ClientState, ConnectionEvent, ConnectionEventHandler, NotificationHandler, PendingRequest,
    RpcRequestHandler, Subscription, error::ClientResult,
};
use dashmap::DashMap;
use ras_jsonrpc_bidirectional_types::{BidirectionalMessage, ConnectionId};
use ras_jsonrpc_types::JsonRpcResponse;
use serde_json::Value;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{debug, error, warn};

pub(super) struct IncomingMessageContext<'a> {
    pub(super) pending_requests: &'a DashMap<Value, PendingRequest>,
    pub(super) subscriptions: &'a DashMap<String, Subscription>,
    pub(super) notification_handlers: &'a DashMap<String, NotificationHandler>,
    pub(super) rpc_request_handlers: &'a DashMap<String, RpcRequestHandler>,
    pub(super) connection_event_handlers: &'a DashMap<String, ConnectionEventHandler>,
    pub(super) connection_id: &'a RwLock<Option<ConnectionId>>,
    pub(super) message_tx: &'a RwLock<Option<mpsc::Sender<BidirectionalMessage>>>,
    pub(super) connected_notify: &'a tokio::sync::Notify,
}

impl Client {
    pub(super) async fn start_message_handler(
        &self,
        mut message_rx: mpsc::Receiver<BidirectionalMessage>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> ClientResult<()> {
        let transport = Arc::clone(&self.transport);
        let pending_requests = Arc::clone(&self.pending_requests);
        let subscriptions = Arc::clone(&self.subscriptions);
        let notification_handlers = Arc::clone(&self.notification_handlers);
        let rpc_request_handlers = Arc::clone(&self.rpc_request_handlers);
        let connection_event_handlers = Arc::clone(&self.connection_event_handlers);
        let connection_id = Arc::clone(&self.connection_id);
        let state = Arc::clone(&self.state);
        let message_tx_clone = Arc::clone(&self.message_tx);
        let connected_notify = Arc::clone(&self.connected_notify);

        spawn_background(async move {
            let mut receive_interval = tokio::time::interval(Duration::from_millis(10));

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        debug!("Message handler received shutdown signal");
                        break;
                    }

                    message = message_rx.recv() => {
                        if let Some(message) = message {
                            let mut transport = transport.write().await;
                            if let Err(e) = transport.send(&message).await {
                                error!("Failed to send message: {}", e);
                            }
                        } else {
                            debug!("Message channel closed");
                            break;
                        }
                    }

                    _ = receive_interval.tick() => {
                        let transport_clone = Arc::clone(&transport);
                        let mut transport = transport_clone.write().await;
                        match transport.receive().await {
                            Ok(Some(message)) => {
                                let context = IncomingMessageContext {
                                    pending_requests: &pending_requests,
                                    subscriptions: &subscriptions,
                                    notification_handlers: &notification_handlers,
                                    rpc_request_handlers: &rpc_request_handlers,
                                    connection_event_handlers: &connection_event_handlers,
                                    connection_id: &connection_id,
                                    message_tx: &message_tx_clone,
                                    connected_notify: &connected_notify,
                                };
                                Self::handle_incoming_message(
                                    message,
                                    context,
                                ).await;
                            }
                            Ok(None) => {
                            }
                            Err(e) => {
                                error!("Failed to receive message: {}", e);
                                *state.write().await = ClientState::Failed;
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    pub(super) async fn handle_incoming_message(
        message: BidirectionalMessage,
        context: IncomingMessageContext<'_>,
    ) {
        match message {
            BidirectionalMessage::Response(response) => {
                if let Some(id) = &response.id {
                    if let Some((_, pending)) = context.pending_requests.remove(id) {
                        let _ = pending.sender.send(response);
                    } else {
                        warn!("Received response for unknown request ID: {:?}", id);
                    }
                }
            }
            BidirectionalMessage::ServerNotification(notification) => {
                if let Some(handler) = context.notification_handlers.get(&notification.method) {
                    handler(&notification.method, &notification.params);
                }
            }
            BidirectionalMessage::Broadcast(broadcast) => {
                if let Some(subscription) = context.subscriptions.get(&broadcast.topic) {
                    (subscription.value().handler)(&broadcast.method, &broadcast.params);
                }
            }
            BidirectionalMessage::ConnectionEstablished {
                connection_id: conn_id,
            } => {
                *context.connection_id.write().await = Some(conn_id);
                // Wake a connect() call waiting on the handshake. notify_one
                // stores a permit, so this works even if connect() has not
                // started waiting yet.
                context.connected_notify.notify_one();
                Self::emit_connection_event_static(
                    ConnectionEvent::Connected {
                        connection_id: conn_id,
                    },
                    context.connection_event_handlers,
                )
                .await;
            }
            BidirectionalMessage::ConnectionClosed { reason, .. } => {
                *context.connection_id.write().await = None;
                Self::emit_connection_event_static(
                    ConnectionEvent::Disconnected { reason },
                    context.connection_event_handlers,
                )
                .await;
            }
            BidirectionalMessage::Request(request) => {
                if let Some(_id) = &request.id {
                    if let Some(handler) = context.rpc_request_handlers.get(&request.method) {
                        debug!("Handling RPC request: {}", request.method);
                        let response = handler(request).await;

                        let response_message = BidirectionalMessage::Response(response);
                        let tx = context.message_tx.read().await.clone();
                        if let Some(tx) = tx
                            && let Err(e) = tx.send(response_message).await
                        {
                            error!("Failed to send RPC response: {}", e);
                        }
                    } else {
                        warn!("No handler registered for RPC method: {}", request.method);
                        let error_response = JsonRpcResponse::error(
                            ras_jsonrpc_types::JsonRpcError::new(
                                -32601,
                                "Method not found".to_string(),
                                None,
                            ),
                            request.id.clone(),
                        );
                        let response_message = BidirectionalMessage::Response(error_response);
                        let tx = context.message_tx.read().await.clone();
                        if let Some(tx) = tx
                            && let Err(e) = tx.send(response_message).await
                        {
                            error!("Failed to send error response: {}", e);
                        }
                    }
                } else {
                    debug!(
                        "Received RPC request without ID (notification): {}",
                        request.method
                    );
                }
            }
            BidirectionalMessage::Pong => {
                debug!("Received pong");
            }
            _ => {
                debug!("Received unhandled message: {:?}", message);
            }
        }
    }

    pub(super) async fn emit_connection_event(&self, event: ConnectionEvent) {
        Self::emit_connection_event_static(event, &self.connection_event_handlers).await;
    }

    pub(super) async fn emit_connection_event_static(
        event: ConnectionEvent,
        handlers: &DashMap<String, ConnectionEventHandler>,
    ) {
        for handler in handlers.iter() {
            handler.value()(event.clone());
        }
    }

    pub(super) async fn start_heartbeat(&self, interval: Duration) {
        let message_tx = Arc::clone(&self.message_tx);
        let state = Arc::clone(&self.state);

        spawn_background(async move {
            let mut heartbeat_interval = tokio::time::interval(interval);
            heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                heartbeat_interval.tick().await;

                let current_state = *state.read().await;
                if current_state != ClientState::Connected {
                    break;
                }

                let tx_guard = message_tx.read().await;
                if let Some(tx) = tx_guard.as_ref() {
                    if tx.send(BidirectionalMessage::Ping).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future);
}

#[cfg(target_arch = "wasm32")]
fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}
