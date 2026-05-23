//! Tests for observability core traits and types

use super::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[test]
fn test_protocol_display() {
    assert_eq!(Protocol::Rest.to_string(), "REST");
    assert_eq!(Protocol::JsonRpc.to_string(), "JSON-RPC");
    assert_eq!(Protocol::WebSocket.to_string(), "WebSocket");
}

#[test]
fn test_protocol_serialization() {
    // Test serialization
    let json = serde_json::to_string(&Protocol::Rest).unwrap();
    assert_eq!(json, "\"Rest\"");

    let json = serde_json::to_string(&Protocol::JsonRpc).unwrap();
    assert_eq!(json, "\"JsonRpc\"");

    // Test deserialization
    let protocol: Protocol = serde_json::from_str("\"WebSocket\"").unwrap();
    assert_eq!(protocol, Protocol::WebSocket);
}

#[test]
fn test_request_context_rest() {
    let ctx = RequestContext::rest("GET", "/api/users");
    assert_eq!(ctx.method, "GET /api/users");
    assert_eq!(ctx.protocol, Protocol::Rest);
    assert!(ctx.metadata.is_empty());
}

#[test]
fn test_request_context_jsonrpc() {
    let ctx = RequestContext::jsonrpc("getUser".to_string());
    assert_eq!(ctx.method, "getUser");
    assert_eq!(ctx.protocol, Protocol::JsonRpc);
    assert!(ctx.metadata.is_empty());
}

#[test]
fn test_request_context_websocket() {
    let ctx = RequestContext::websocket("sendMessage");
    assert_eq!(ctx.method, "sendMessage");
    assert_eq!(ctx.protocol, Protocol::WebSocket);
    assert!(ctx.metadata.is_empty());
}

#[test]
fn test_request_context_with_metadata() {
    let ctx = RequestContext::rest("POST", "/api/users")
        .with_metadata("request_id", "123")
        .with_metadata("version", "v1");

    assert_eq!(ctx.metadata.get("request_id"), Some(&"123".to_string()));
    assert_eq!(ctx.metadata.get("version"), Some(&"v1".to_string()));
}

#[test]
fn test_user_agent_extraction() {
    let mut headers = HeaderMap::new();

    // Test with user agent
    headers.insert("user-agent", "Mozilla/5.0".parse().unwrap());
    assert_eq!(extractors::user_agent(&headers), "Mozilla/5.0");

    // Test without user agent
    headers.clear();
    assert_eq!(extractors::user_agent(&headers), "unknown");
}

#[test]
fn test_user_attributes_authenticated() {
    let user = AuthenticatedUser {
        user_id: "user123".to_string(),
        permissions: vec!["read".to_string(), "write".to_string(), "admin".to_string()]
            .into_iter()
            .collect(),
        metadata: None,
    };

    let attrs = extractors::user_attributes(Some(&user));

    assert_eq!(attrs.get("user_id"), Some(&"user123".to_string()));
    assert_eq!(attrs.get("authenticated"), Some(&"true".to_string()));
    assert_eq!(attrs.get("has_admin"), Some(&"true".to_string()));

    // Permissions order might vary, so check if both exist
    let perms = attrs.get("permissions").unwrap();
    assert!(perms.contains("read"));
    assert!(perms.contains("write"));
    assert!(perms.contains("admin"));
}

#[test]
fn test_user_attributes_anonymous() {
    let attrs = extractors::user_attributes(None);

    assert_eq!(attrs.get("user_id"), Some(&"anonymous".to_string()));
    assert_eq!(attrs.get("authenticated"), Some(&"false".to_string()));
}

// Mock implementations for testing traits
struct MockUsageTracker {
    calls: Arc<Mutex<Vec<(String, String, String)>>>, // method, protocol, user_id
}

impl MockUsageTracker {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl UsageTracker for MockUsageTracker {
    async fn track_request(
        &self,
        _headers: &HeaderMap,
        user: Option<&AuthenticatedUser>,
        context: &RequestContext,
    ) {
        let user_id = user
            .map(|u| u.user_id.clone())
            .unwrap_or("anonymous".to_string());
        self.calls.lock().await.push((
            context.method.clone(),
            context.protocol.to_string(),
            user_id,
        ));
    }
}

struct MockMethodDurationTracker {
    durations: Arc<Mutex<Vec<(String, Duration)>>>, // method, duration
}

impl MockMethodDurationTracker {
    fn new() -> Self {
        Self {
            durations: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl MethodDurationTracker for MockMethodDurationTracker {
    async fn track_duration(
        &self,
        context: &RequestContext,
        _user: Option<&AuthenticatedUser>,
        duration: Duration,
    ) {
        self.durations
            .lock()
            .await
            .push((context.method.clone(), duration));
    }
}

struct MockServiceMetrics {
    requests_started: Arc<Mutex<Vec<(String, String)>>>, // method, protocol
    requests_completed: Arc<Mutex<Vec<(String, String, bool)>>>, // method, protocol, success
    method_durations: Arc<Mutex<Vec<(String, Duration)>>>, // method, duration
}

impl MockServiceMetrics {
    fn new() -> Self {
        Self {
            requests_started: Arc::new(Mutex::new(Vec::new())),
            requests_completed: Arc::new(Mutex::new(Vec::new())),
            method_durations: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ServiceMetrics for MockServiceMetrics {
    fn increment_requests_started(&self, context: &RequestContext) {
        self.requests_started
            .try_lock()
            .unwrap()
            .push((context.method.clone(), context.protocol.to_string()));
    }

    fn increment_requests_completed(&self, context: &RequestContext, success: bool) {
        self.requests_completed.try_lock().unwrap().push((
            context.method.clone(),
            context.protocol.to_string(),
            success,
        ));
    }

    fn record_method_duration(&self, context: &RequestContext, duration: Duration) {
        self.method_durations
            .try_lock()
            .unwrap()
            .push((context.method.clone(), duration));
    }
}

#[tokio::test]
async fn test_usage_tracker_trait() {
    let tracker = MockUsageTracker::new();
    let user = AuthenticatedUser {
        user_id: "test_user".to_string(),
        permissions: vec!["read".to_string()].into_iter().collect(),
        metadata: None,
    };
    let context = RequestContext::jsonrpc("testMethod".to_string());

    tracker
        .track_request(&HeaderMap::new(), Some(&user), &context)
        .await;

    let calls = tracker.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        (
            "testMethod".to_string(),
            "JSON-RPC".to_string(),
            "test_user".to_string()
        )
    );
}

#[tokio::test]
async fn test_method_duration_tracker_trait() {
    let tracker = MockMethodDurationTracker::new();
    let context = RequestContext::rest("POST", "/api/data");
    let duration = Duration::from_millis(150);

    tracker.track_duration(&context, None, duration).await;

    let durations = tracker.durations.lock().await;
    assert_eq!(durations.len(), 1);
    assert_eq!(durations[0].0, "POST /api/data");
    assert_eq!(durations[0].1, duration);
}

#[test]
fn test_service_metrics_trait() {
    let metrics = MockServiceMetrics::new();
    let context = RequestContext::rest("GET", "/health");

    // Test request started
    metrics.increment_requests_started(&context);
    assert_eq!(metrics.requests_started.try_lock().unwrap().len(), 1);

    // Test request completed
    metrics.increment_requests_completed(&context, true);
    assert_eq!(metrics.requests_completed.try_lock().unwrap().len(), 1);
    assert!(metrics.requests_completed.try_lock().unwrap()[0].2);

    // Test method duration
    let duration = Duration::from_secs(1);
    metrics.record_method_duration(&context, duration);
    assert_eq!(metrics.method_durations.try_lock().unwrap().len(), 1);
}

#[tokio::test]
async fn test_observability_builder_with_usage_tracker() {
    let call_count = Arc::new(Mutex::new(0));
    let call_count_clone = call_count.clone();

    let builder =
        ObservabilityBuilder::new().with_usage_tracker(move |_headers, _user, _context| {
            let count = call_count_clone.clone();
            async move {
                *count.lock().await += 1;
            }
        });

    let config = builder.build();
    assert!(config.usage_tracker.is_some());

    // Test that the tracker function works
    let tracker_fn = config.usage_tracker.unwrap();
    let fut = tracker_fn(
        HeaderMap::new(),
        None,
        RequestContext::jsonrpc("test".to_string()),
    );
    fut.await;

    assert_eq!(*call_count.lock().await, 1);
}

#[tokio::test]
async fn test_observability_builder_with_duration_tracker() {
    let duration_sum = Arc::new(Mutex::new(Duration::ZERO));
    let duration_sum_clone = duration_sum.clone();

    let builder = ObservabilityBuilder::new().with_method_duration_tracker(
        move |_context, _user, duration| {
            let sum = duration_sum_clone.clone();
            async move {
                *sum.lock().await += duration;
            }
        },
    );

    let config = builder.build();
    assert!(config.duration_tracker.is_some());

    // Test that the tracker function works
    let tracker_fn = config.duration_tracker.unwrap();
    let test_duration = Duration::from_millis(100);
    let fut = tracker_fn(RequestContext::rest("GET", "/test"), None, test_duration);
    fut.await;

    assert_eq!(*duration_sum.lock().await, test_duration);
}

#[test]
fn test_observability_builder_default() {
    let builder = ObservabilityBuilder::default();
    let config = builder.build();

    assert!(config.usage_tracker.is_none());
    assert!(config.duration_tracker.is_none());
}

#[test]
fn test_request_context_cloning() {
    let ctx = RequestContext::rest("PUT", "/api/resource").with_metadata("key", "value");

    let cloned = ctx.clone();
    assert_eq!(cloned.method, ctx.method);
    assert_eq!(cloned.protocol, ctx.protocol);
    assert_eq!(cloned.metadata, ctx.metadata);
}

#[test]
fn test_protocol_equality() {
    assert_eq!(Protocol::Rest, Protocol::Rest);
    assert_ne!(Protocol::Rest, Protocol::JsonRpc);
    assert_ne!(Protocol::JsonRpc, Protocol::WebSocket);
}
