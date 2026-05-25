//! Tests for OpenTelemetry observability implementation

use super::*;
use axum::http::HeaderMap;
use axum_test::TestServer;
use opentelemetry::global;
use prometheus::Registry;
use ras_observability_core::{Protocol, RequestContext};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

#[test]
fn test_otel_metrics_creation() {
    // Create a test meter
    let meter = global::meter("test_service");
    let metrics = OtelMetrics::new(&meter);

    // Verify metrics are created (we can't directly test their properties,
    // but we can verify they don't panic when used)
    let context = RequestContext::rest("GET", "/test");
    metrics.increment_requests_started(&context);
    metrics.increment_requests_completed(&context, true);
    metrics.record_method_duration(&context, Duration::from_millis(100));
}

#[test]
fn test_service_metrics_implementation() {
    let meter = global::meter("test_metrics");
    let metrics = OtelMetrics::new(&meter);

    // Test various request contexts
    let rest_ctx = RequestContext::rest("POST", "/api/users");
    let jsonrpc_ctx = RequestContext::jsonrpc("createUser".to_string());
    let ws_ctx = RequestContext {
        method: "subscribe".to_string(),
        protocol: Protocol::WebSocket,
        metadata: HashMap::new(),
    };

    // Test increment_requests_started
    metrics.increment_requests_started(&rest_ctx);
    metrics.increment_requests_started(&jsonrpc_ctx);
    metrics.increment_requests_started(&ws_ctx);

    // Test increment_requests_completed with different success states
    metrics.increment_requests_completed(&rest_ctx, true);
    metrics.increment_requests_completed(&jsonrpc_ctx, false);
    metrics.increment_requests_completed(&ws_ctx, true);

    // Test record_method_duration with various durations
    metrics.record_method_duration(&rest_ctx, Duration::from_millis(50));
    metrics.record_method_duration(&jsonrpc_ctx, Duration::from_secs(1));
    metrics.record_method_duration(&ws_ctx, Duration::from_micros(500));
}

#[tokio::test]
async fn test_otel_usage_tracker() {
    let meter = global::meter("test_usage_tracker");
    let metrics = Arc::new(OtelMetrics::new(&meter));
    let tracker = OtelUsageTracker::new(metrics.clone());

    // Test with authenticated user
    let user = AuthenticatedUser {
        user_id: "test_user_123".to_string(),
        permissions: vec!["read".to_string(), "write".to_string()]
            .into_iter()
            .collect(),
        metadata: None,
    };

    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "TestClient/1.0".parse().unwrap());

    let context =
        RequestContext::jsonrpc("getStatus".to_string()).with_metadata("request_id", "req-123");

    // Should not panic and should increment metrics
    tracker.track_request(&headers, Some(&user), &context).await;

    // Test with anonymous user
    tracker.track_request(&headers, None, &context).await;
}

#[tokio::test]
async fn test_otel_method_duration_tracker() {
    let meter = global::meter("test_duration_tracker");
    let metrics = Arc::new(OtelMetrics::new(&meter));
    let tracker = OtelMethodDurationTracker::new(metrics.clone());

    let context = RequestContext::rest("DELETE", "/api/items/123");
    let user = AuthenticatedUser {
        user_id: "admin".to_string(),
        permissions: vec!["admin".to_string()].into_iter().collect(),
        metadata: None,
    };

    // Test with authenticated user
    tracker
        .track_duration(&context, Some(&user), Duration::from_millis(250))
        .await;

    // Test with anonymous user
    tracker
        .track_duration(&context, None, Duration::from_millis(100))
        .await;
}

#[tokio::test]
async fn test_otel_setup_builder() {
    // Test basic setup
    let setup = OtelSetupBuilder::new("test_service")
        .build()
        .expect("Failed to build OTel setup");

    assert_eq!(setup.service_name, "test_service");

    // Verify components are created
    let _usage_tracker = setup.usage_tracker();
    let _duration_tracker = setup.method_duration_tracker();
    let _metrics = setup.metrics();
    let _router = setup.metrics_router();
}

#[tokio::test]
async fn test_otel_setup_with_custom_registry() {
    let custom_registry = Registry::new();

    let setup = OtelSetupBuilder::new("custom_service")
        .with_prometheus_registry(custom_registry)
        .build()
        .expect("Failed to build OTel setup with custom registry");

    assert_eq!(setup.service_name, "custom_service");
}

#[tokio::test]
async fn test_standard_setup() {
    let setup = standard_setup("standard_service").expect("Failed to create standard setup");

    assert_eq!(setup.service_name, "standard_service");
}

#[tokio::test]
async fn test_metrics_handler() {
    // Use test-specific setup to avoid conflicts
    let setup = OtelSetupBuilder::new("test_metrics_handler")
        .build()
        .expect("Failed to create setup");

    // Create a test app with the metrics endpoint
    let app = setup.metrics_router();

    // Make a request to the metrics endpoint
    let server = TestServer::builder().mock_transport().build(app).unwrap();
    let response = server.get("/metrics").await;

    // Basic checks that the endpoint works
    response.assert_status_ok();
    response.assert_header("content-type", "text/plain; version=0.0.4");

    // Check that we get a valid Prometheus response
    let body = response.text();
    assert!(!body.is_empty(), "Metrics response should not be empty");

    // The response should contain Prometheus format markers
    assert!(
        body.contains("# HELP") || body.contains("# TYPE") || body.contains("target_info"),
        "Response should be in Prometheus format. Got:\n{}",
        body
    );
}

#[tokio::test]
async fn test_usage_tracker_with_various_headers() {
    let meter = global::meter("header_test");
    let metrics = Arc::new(OtelMetrics::new(&meter));
    let tracker = OtelUsageTracker::new(metrics);

    let context = RequestContext::rest("POST", "/api/data");

    // Test with various user agents
    let mut headers = HeaderMap::new();
    headers.insert(
        "user-agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64)".parse().unwrap(),
    );
    tracker.track_request(&headers, None, &context).await;

    // Test with missing user agent
    headers.clear();
    tracker.track_request(&headers, None, &context).await;

    // Test with invalid user agent (should still work)
    headers.insert("user-agent", "".parse().unwrap());
    tracker.track_request(&headers, None, &context).await;
}

#[tokio::test]
async fn test_concurrent_metric_updates() {
    let meter = global::meter("concurrent_test");
    let metrics = Arc::new(OtelMetrics::new(&meter));

    // Spawn multiple tasks updating metrics concurrently
    let mut handles = vec![];

    for i in 0..10 {
        let metrics = metrics.clone();
        let handle = tokio::spawn(async move {
            let context = RequestContext::rest("GET", &format!("/api/resource/{}", i));

            for _ in 0..100 {
                metrics.increment_requests_started(&context);
                sleep(Duration::from_micros(10)).await;
                metrics.increment_requests_completed(&context, i % 2 == 0);
                metrics.record_method_duration(&context, Duration::from_millis(i as u64));
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[test]
fn test_otel_metrics_clone() {
    let meter = global::meter("clone_test");
    let metrics = OtelMetrics::new(&meter);
    let cloned = metrics.clone();

    // Both should work independently
    let context = RequestContext::jsonrpc("test".to_string());
    metrics.increment_requests_started(&context);
    cloned.increment_requests_started(&context);
}

#[test]
fn test_arc_wrapping() {
    let meter = global::meter("arc_test");
    let metrics = Arc::new(OtelMetrics::new(&meter));

    // Test that Arc<OtelMetrics> can be used
    let usage_tracker = OtelUsageTracker::new(metrics.clone());
    let duration_tracker = OtelMethodDurationTracker::new(metrics.clone());

    // Verify they're using the same underlying metrics
    let _tracker_clone = usage_tracker.clone();
    let _duration_clone = duration_tracker.clone();
}

#[tokio::test]
async fn test_metadata_in_context() {
    let meter = global::meter("metadata_test");
    let metrics = Arc::new(OtelMetrics::new(&meter));
    let tracker = OtelUsageTracker::new(metrics.clone());

    let context = RequestContext::jsonrpc("complexMethod".to_string())
        .with_metadata("version", "2.0")
        .with_metadata("client_id", "mobile-app")
        .with_metadata("trace_id", "abc123");

    let headers = HeaderMap::new();
    tracker.track_request(&headers, None, &context).await;

    // Metadata should be available but not necessarily used in metrics
    // (to avoid cardinality explosion)
    assert_eq!(context.metadata.len(), 3);
}

#[tokio::test]
async fn test_various_duration_scales() {
    let meter = global::meter("duration_scale_test");
    let metrics = Arc::new(OtelMetrics::new(&meter));
    let tracker = OtelMethodDurationTracker::new(metrics.clone());

    let context = RequestContext::rest("GET", "/api/fast");

    // Test various duration scales
    tracker
        .track_duration(&context, None, Duration::from_nanos(100))
        .await;
    tracker
        .track_duration(&context, None, Duration::from_micros(500))
        .await;
    tracker
        .track_duration(&context, None, Duration::from_millis(50))
        .await;
    tracker
        .track_duration(&context, None, Duration::from_secs(2))
        .await;
}

#[test]
fn test_protocol_usage_in_metrics() {
    let meter = global::meter("protocol_test");
    let metrics = OtelMetrics::new(&meter);

    // Test that all protocols are handled correctly
    for protocol in [Protocol::Rest, Protocol::JsonRpc, Protocol::WebSocket] {
        let context = RequestContext {
            method: "test_method".to_string(),
            protocol,
            metadata: HashMap::new(),
        };

        metrics.increment_requests_started(&context);
        metrics.increment_requests_completed(&context, true);
        metrics.record_method_duration(&context, Duration::from_millis(100));
    }
}
