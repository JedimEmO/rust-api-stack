//! Integration tests for observability with real services

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
};
use axum_test::TestServer;
use ras_auth_core::AuthenticatedUser;
use ras_observability_core::{MethodDurationTracker, RequestContext, ServiceMetrics, UsageTracker};
use ras_observability_otel::{OtelMethodDurationTracker, OtelSetupBuilder, OtelUsageTracker};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};
use tokio::{
    sync::{Mutex, MutexGuard},
    time::{Duration, sleep},
};

static OTEL_TEST_LOCK: Mutex<()> = Mutex::const_new(());

async fn otel_test_guard() -> MutexGuard<'static, ()> {
    OTEL_TEST_LOCK.lock().await
}

#[derive(Clone)]
struct AppState {
    usage_tracker: OtelUsageTracker,
    duration_tracker: OtelMethodDurationTracker,
    metrics: Arc<ras_observability_otel::OtelMetrics>,
}

#[derive(Serialize, Deserialize)]
struct CreateUserRequest {
    username: String,
    email: String,
}

#[derive(Serialize, Deserialize)]
struct UserResponse {
    id: String,
    username: String,
}

// Example REST endpoint with observability
async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, StatusCode> {
    let start = Instant::now();
    let context = RequestContext::rest("POST", "/api/users");

    // Track request start
    state
        .usage_tracker
        .track_request(&headers, None, &context)
        .await;
    state.metrics.increment_requests_started(&context);

    // Create response
    let response = UserResponse {
        id: "user-123".to_string(),
        username: payload.username,
    };

    // Track completion
    let duration = start.elapsed();
    state
        .duration_tracker
        .track_duration(&context, None, duration)
        .await;
    state.metrics.increment_requests_completed(&context, true);

    Ok(Json(response))
}

// Example health check endpoint
async fn health_check(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    let start = Instant::now();
    let context = RequestContext::rest("GET", "/health");

    // Track the request
    state
        .usage_tracker
        .track_request(&headers, None, &context)
        .await;
    state.metrics.increment_requests_started(&context);

    // Track completion
    let duration = start.elapsed();
    state
        .duration_tracker
        .track_duration(&context, None, duration)
        .await;
    state.metrics.increment_requests_completed(&context, true);

    StatusCode::OK
}

// Example authenticated endpoint
async fn get_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserResponse>, StatusCode> {
    let start = Instant::now();
    let context = RequestContext::rest("GET", "/api/profile");

    // Simulate authenticated user
    let user = AuthenticatedUser {
        user_id: "auth-user-456".to_string(),
        permissions: vec!["read_profile".to_string(), "write_profile".to_string()]
            .into_iter()
            .collect(),
        metadata: None,
    };

    // Track with authenticated user
    state
        .usage_tracker
        .track_request(&headers, Some(&user), &context)
        .await;
    state.metrics.increment_requests_started(&context);

    let response = UserResponse {
        id: user.user_id.clone(),
        username: "test_user".to_string(),
    };

    // Track completion
    let duration = start.elapsed();
    state
        .duration_tracker
        .track_duration(&context, Some(&user), duration)
        .await;
    state.metrics.increment_requests_completed(&context, true);

    Ok(Json(response))
}

#[tokio::test]
async fn test_full_service_integration() {
    let _guard = otel_test_guard().await;

    // Set up observability
    let setup = OtelSetupBuilder::new("integration_test_service")
        .build()
        .expect("Failed to set up OpenTelemetry");

    // Create app state
    let state = AppState {
        usage_tracker: setup.usage_tracker(),
        duration_tracker: setup.method_duration_tracker(),
        metrics: setup.metrics(),
    };

    // Build the application
    let api_routes = Router::new()
        .route("/health", get(health_check))
        .route("/api/users", post(create_user))
        .route("/api/profile", get(get_profile))
        .with_state(state);

    let app = Router::new()
        .merge(api_routes)
        .merge(setup.metrics_router());

    // Create test server
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    // Test health check
    let response = server.get("/health").await;
    response.assert_status_ok();

    // Test user creation
    let create_request = CreateUserRequest {
        username: "newuser".to_string(),
        email: "newuser@example.com".to_string(),
    };

    let response = server.post("/api/users").json(&create_request).await;

    response.assert_status_ok();
    let user: UserResponse = response.json();
    assert_eq!(user.username, "newuser");

    // Test authenticated endpoint
    let response = server.get("/api/profile").await;
    response.assert_status_ok();

    // Make multiple sequential requests to generate more metrics
    for i in 0..5 {
        // Mix of different endpoints
        if i % 2 == 0 {
            server.get("/health").await;
        } else {
            let req = CreateUserRequest {
                username: format!("user{}", i),
                email: format!("user{}@test.com", i),
            };
            server.post("/api/users").json(&req).await;
        }
    }

    // Force flush metrics to ensure they're recorded
    setup.force_flush().expect("Failed to flush metrics");
    // Small delay to allow Prometheus registry to update
    sleep(Duration::from_millis(10)).await;

    // Test metrics endpoint
    let response = server.get("/metrics").await;
    response.assert_status_ok();

    let metrics_text = response.text();

    // Verify metrics are present
    assert!(metrics_text.contains("requests_started_total"));
    assert!(metrics_text.contains("requests_completed_total"));
    assert!(metrics_text.contains("method_duration_milliseconds"));

    // Verify specific labels are present
    assert!(metrics_text.contains("method=\"GET /health\""));
    assert!(metrics_text.contains("method=\"POST /api/users\""));
    assert!(metrics_text.contains("method=\"GET /api/profile\""));
    assert!(metrics_text.contains("protocol=\"REST\""));
    assert!(metrics_text.contains("success=\"true\""));
}

#[tokio::test]
async fn test_jsonrpc_protocol_tracking() {
    let _guard = otel_test_guard().await;

    let setup = OtelSetupBuilder::new("jsonrpc_test_service")
        .build()
        .expect("Failed to set up OpenTelemetry");

    let metrics = setup.metrics();
    let usage_tracker = setup.usage_tracker();
    let duration_tracker = setup.method_duration_tracker();

    // Simulate JSON-RPC requests
    let headers = HeaderMap::new();
    let jsonrpc_methods = ["getUser", "createUser", "updateUser", "deleteUser"];

    for method in &jsonrpc_methods {
        let context = RequestContext::jsonrpc(method.to_string());

        // Track request
        usage_tracker.track_request(&headers, None, &context).await;
        metrics.increment_requests_started(&context);

        // Track completion
        duration_tracker
            .track_duration(&context, None, Duration::from_millis(10))
            .await;
        metrics.increment_requests_completed(&context, true);
    }

    // Force flush metrics to ensure they're recorded
    setup.force_flush().expect("Failed to flush metrics");
    // Small delay to allow Prometheus registry to update
    sleep(Duration::from_millis(10)).await;

    // Create metrics endpoint to verify
    let app = setup.metrics_router();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.get("/metrics").await;
    let metrics_text = response.text();

    // Verify JSON-RPC methods are tracked
    for method in &jsonrpc_methods {
        assert!(metrics_text.contains(&format!("method=\"{}\"", method)));
    }
    assert!(metrics_text.contains("protocol=\"JSON-RPC\""));
}

#[tokio::test]
async fn test_websocket_protocol_tracking() {
    let _guard = otel_test_guard().await;

    let setup = OtelSetupBuilder::new("websocket_test_service")
        .build()
        .expect("Failed to set up OpenTelemetry");

    let metrics = setup.metrics();
    let _headers = HeaderMap::new();

    // Simulate WebSocket operations
    let ws_operations = ["connect", "subscribe", "publish", "disconnect"];

    for operation in &ws_operations {
        let context =
            RequestContext::websocket(*operation).with_metadata("connection_id", "ws-123");

        metrics.increment_requests_started(&context);
        metrics.record_method_duration(&context, Duration::from_millis(5));
        metrics.increment_requests_completed(&context, true);
    }

    // Force flush metrics to ensure they're recorded
    setup.force_flush().expect("Failed to flush metrics");
    // Small delay to allow Prometheus registry to update
    sleep(Duration::from_millis(10)).await;

    // Verify metrics
    let app = setup.metrics_router();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.get("/metrics").await;
    let metrics_text = response.text();

    // Check WebSocket operations are tracked
    assert!(metrics_text.contains("protocol=\"WebSocket\""));
    for operation in &ws_operations {
        assert!(metrics_text.contains(&format!("method=\"{}\"", operation)));
    }
}

#[tokio::test]
async fn test_error_scenarios() {
    let _guard = otel_test_guard().await;

    let setup = OtelSetupBuilder::new("error_test_service")
        .build()
        .expect("Failed to set up OpenTelemetry");

    let metrics = setup.metrics();

    // Test various failure scenarios
    let failure_contexts = vec![
        RequestContext::rest("POST", "/api/users"), // Bad request
        RequestContext::rest("GET", "/api/unauthorized"), // Unauthorized
        RequestContext::jsonrpc("invalidMethod".to_string()), // Method not found
    ];

    for context in failure_contexts {
        metrics.increment_requests_started(&context);

        // Simulate varying processing times for failures
        let duration = match context.method.as_str() {
            "POST /api/users" => Duration::from_millis(20),
            "GET /api/unauthorized" => Duration::from_millis(5),
            _ => Duration::from_millis(1),
        };

        metrics.record_method_duration(&context, duration);
        metrics.increment_requests_completed(&context, false); // Mark as failed
    }

    // Force flush metrics to ensure they're recorded
    setup.force_flush().expect("Failed to flush metrics");
    // Small delay to allow Prometheus registry to update
    sleep(Duration::from_millis(10)).await;

    // Verify failure metrics
    let app = setup.metrics_router();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.get("/metrics").await;
    let metrics_text = response.text();

    // Should have both success="true" and success="false" metrics
    assert!(metrics_text.contains("success=\"false\""));
}

#[tokio::test]
async fn test_high_cardinality_protection() {
    let _guard = otel_test_guard().await;

    let setup = OtelSetupBuilder::new("cardinality_test_service")
        .build()
        .expect("Failed to set up OpenTelemetry");

    let metrics = setup.metrics();
    let usage_tracker = setup.usage_tracker();

    // Create many requests with different metadata that should NOT increase metric cardinality
    for i in 0..100 {
        let context = RequestContext::rest("GET", "/api/items")
            .with_metadata("item_id", format!("{}", i))
            .with_metadata("user_id", format!("user-{}", i))
            .with_metadata("session_id", format!("session-{}", i));

        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", format!("req-{}", i).parse().unwrap());

        let user = AuthenticatedUser {
            user_id: format!("user-{}", i),
            permissions: vec![format!("perm-{}", i % 10)].into_iter().collect(),
            metadata: None,
        };

        usage_tracker
            .track_request(&headers, Some(&user), &context)
            .await;
        metrics.increment_requests_started(&context);
        metrics.record_method_duration(&context, Duration::from_millis(i as u64 % 100));
        metrics.increment_requests_completed(&context, true);
    }

    // Metrics should only have limited cardinality based on method and protocol
    // not on user_id, item_id, or other high-cardinality fields
    let app = setup.metrics_router();
    let server = TestServer::builder().mock_transport().build(app).unwrap();

    let response = server.get("/metrics").await;
    let metrics_text = response.text();

    // Should not contain any of the high-cardinality values
    assert!(!metrics_text.contains("user-50"));
    assert!(!metrics_text.contains("item_id"));
    assert!(!metrics_text.contains("session_id"));
    assert!(!metrics_text.contains("req-"));
}
