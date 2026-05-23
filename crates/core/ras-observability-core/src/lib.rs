//! Core observability traits and types for Rust Agent Stack
//!
//! This crate provides protocol-agnostic abstractions for metrics collection,
//! usage tracking, and observability across REST and JSON-RPC services.

use async_trait::async_trait;
use axum::http::HeaderMap;
use ras_auth_core::AuthenticatedUser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Protocol type for request context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Protocol {
    /// REST/HTTP protocol
    Rest,
    /// JSON-RPC protocol
    JsonRpc,
    /// WebSocket protocol (for bidirectional communication)
    WebSocket,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Rest => write!(f, "REST"),
            Protocol::JsonRpc => write!(f, "JSON-RPC"),
            Protocol::WebSocket => write!(f, "WebSocket"),
        }
    }
}

/// Common request context that can represent both REST and JSON-RPC requests
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// The method being called
    /// - For REST: "GET /users" or "POST /api/v1/users"
    /// - For JSON-RPC: "getUser" or "createUser"
    pub method: String,

    /// The protocol being used
    pub protocol: Protocol,

    /// Additional metadata about the request
    /// - For REST: could include path parameters, query strings
    /// - For JSON-RPC: could include request ID, version
    pub metadata: HashMap<String, String>,
}

impl RequestContext {
    /// Create a new REST request context
    pub fn rest(http_method: &str, path: &str) -> Self {
        Self {
            method: format!("{} {}", http_method, path),
            protocol: Protocol::Rest,
            metadata: HashMap::new(),
        }
    }

    /// Create a new JSON-RPC request context
    pub fn jsonrpc(method: String) -> Self {
        Self {
            method,
            protocol: Protocol::JsonRpc,
            metadata: HashMap::new(),
        }
    }

    /// Create a new WebSocket request context
    pub fn websocket(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            protocol: Protocol::WebSocket,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Type alias for async usage tracking function
pub type UsageTrackerFn = Box<
    dyn Fn(
            HeaderMap,
            Option<AuthenticatedUser>,
            RequestContext,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Type alias for async method duration tracking function
pub type MethodDurationTrackerFn = Box<
    dyn Fn(
            RequestContext,
            Option<AuthenticatedUser>,
            Duration,
        ) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Trait for tracking request usage
#[async_trait]
pub trait UsageTracker: Send + Sync {
    /// Track a request before it's processed
    async fn track_request(
        &self,
        headers: &HeaderMap,
        user: Option<&AuthenticatedUser>,
        context: &RequestContext,
    );
}

/// Trait for tracking method execution duration
#[async_trait]
pub trait MethodDurationTracker: Send + Sync {
    /// Track the duration of a method execution
    async fn track_duration(
        &self,
        context: &RequestContext,
        user: Option<&AuthenticatedUser>,
        duration: Duration,
    );
}

/// Common metrics that should be tracked across all services
pub trait ServiceMetrics: Send + Sync {
    /// Increment the count of requests started
    fn increment_requests_started(&self, context: &RequestContext);

    /// Increment the count of requests completed
    fn increment_requests_completed(&self, context: &RequestContext, success: bool);

    /// Record the duration of a method execution
    fn record_method_duration(&self, context: &RequestContext, duration: Duration);
}

/// Builder for configuring observability
pub struct ObservabilityBuilder {
    usage_tracker: Option<UsageTrackerFn>,
    duration_tracker: Option<MethodDurationTrackerFn>,
}

impl ObservabilityBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            usage_tracker: None,
            duration_tracker: None,
        }
    }

    /// Set the usage tracker
    pub fn with_usage_tracker<F, Fut>(mut self, tracker: F) -> Self
    where
        F: Fn(HeaderMap, Option<AuthenticatedUser>, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.usage_tracker = Some(Box::new(move |headers, user, context| {
            Box::pin(tracker(headers, user, context))
        }));
        self
    }

    /// Set the method duration tracker
    pub fn with_method_duration_tracker<F, Fut>(mut self, tracker: F) -> Self
    where
        F: Fn(RequestContext, Option<AuthenticatedUser>, Duration) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.duration_tracker = Some(Box::new(move |context, user, duration| {
            Box::pin(tracker(context, user, duration))
        }));
        self
    }

    /// Build the observability configuration
    pub fn build(self) -> ObservabilityConfig {
        ObservabilityConfig {
            usage_tracker: self.usage_tracker,
            duration_tracker: self.duration_tracker,
        }
    }
}

impl Default for ObservabilityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for observability
pub struct ObservabilityConfig {
    pub usage_tracker: Option<UsageTrackerFn>,
    pub duration_tracker: Option<MethodDurationTrackerFn>,
}

/// Helper functions for extracting common attributes from requests
pub mod extractors {
    use super::*;

    /// Extract user agent from headers
    pub fn user_agent(headers: &HeaderMap) -> String {
        headers
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown")
            .to_string()
    }

    /// Extract common user attributes
    pub fn user_attributes(user: Option<&AuthenticatedUser>) -> HashMap<String, String> {
        let mut attrs = HashMap::new();

        if let Some(user) = user {
            attrs.insert("user_id".to_string(), user.user_id.clone());
            attrs.insert("authenticated".to_string(), "true".to_string());
            attrs.insert(
                "permissions".to_string(),
                user.permissions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            );
            attrs.insert(
                "has_admin".to_string(),
                user.permissions.contains("admin").to_string(),
            );
        } else {
            attrs.insert("user_id".to_string(), "anonymous".to_string());
            attrs.insert("authenticated".to_string(), "false".to_string());
        }

        attrs
    }
}

// Re-export commonly used types
pub use extractors::{user_agent, user_attributes};

#[cfg(test)]
mod tests;
