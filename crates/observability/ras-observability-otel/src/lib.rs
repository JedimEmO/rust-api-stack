//! OpenTelemetry implementation for Rust Agent Stack observability
//!
//! This crate provides an OpenTelemetry implementation with Prometheus export
//! support and standard metric definitions.

use async_trait::async_trait;
use axum::{
    Router, body::Body, extract::State, http::StatusCode, response::Response, routing::get,
};
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram, Meter},
};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use ras_auth_core::AuthenticatedUser;
use ras_observability_core::{
    MethodDurationTracker, RequestContext, ServiceMetrics, UsageTracker, extractors::user_agent,
};
use std::{sync::Arc, time::Duration};
use tracing::info;

/// Standard metrics for services using OpenTelemetry
#[derive(Clone)]
pub struct OtelMetrics {
    requests_started: Counter<u64>,
    requests_completed: Counter<u64>,
    method_duration: Histogram<f64>,
}

impl OtelMetrics {
    /// Create new metrics with a given meter
    pub fn new(meter: &Meter) -> Self {
        Self {
            requests_started: meter
                .u64_counter("requests_started")
                .with_description("Total number of requests started")
                .with_unit("requests")
                .build(),
            requests_completed: meter
                .u64_counter("requests_completed")
                .with_description("Total number of requests completed")
                .with_unit("requests")
                .build(),
            method_duration: meter
                .f64_histogram("method_duration_milliseconds")
                .with_description("Duration of method execution in milliseconds")
                .with_unit("milliseconds")
                .build(),
        }
    }
}

impl ServiceMetrics for OtelMetrics {
    fn increment_requests_started(&self, context: &RequestContext) {
        let attributes = vec![
            KeyValue::new("method", context.method.clone()),
            KeyValue::new("protocol", context.protocol.to_string()),
        ];

        self.requests_started.add(1, &attributes);
    }

    fn increment_requests_completed(&self, context: &RequestContext, success: bool) {
        let attributes = vec![
            KeyValue::new("method", context.method.clone()),
            KeyValue::new("protocol", context.protocol.to_string()),
            KeyValue::new("success", success.to_string()),
        ];

        self.requests_completed.add(1, &attributes);
    }

    fn record_method_duration(&self, context: &RequestContext, duration: Duration) {
        // Duration metrics should only include method and protocol to avoid cardinality explosion
        let attributes = vec![
            KeyValue::new("method", context.method.clone()),
            KeyValue::new("protocol", context.protocol.to_string()),
        ];

        self.method_duration
            .record(duration.as_secs_f64() * 1000.0, &attributes);
    }
}

/// Usage tracker implementation that logs and records metrics
#[derive(Clone)]
pub struct OtelUsageTracker {
    metrics: Arc<OtelMetrics>,
}

impl OtelUsageTracker {
    pub fn new(metrics: Arc<OtelMetrics>) -> Self {
        Self { metrics }
    }
}

#[async_trait]
impl UsageTracker for OtelUsageTracker {
    async fn track_request(
        &self,
        headers: &axum::http::HeaderMap,
        user: Option<&AuthenticatedUser>,
        context: &RequestContext,
    ) {
        let user_agent = user_agent(headers);

        // Log the request
        match user {
            Some(u) => {
                info!(
                    protocol = %context.protocol,
                    method = %context.method,
                    user_id = %u.user_id,
                    permissions = ?u.permissions,
                    user_agent = %user_agent,
                    "Request started"
                );
            }
            None => {
                info!(
                    protocol = %context.protocol,
                    method = %context.method,
                    user_id = "anonymous",
                    user_agent = %user_agent,
                    "Request started"
                );
            }
        }

        // Record metrics
        self.metrics.increment_requests_started(context);
    }
}

/// Method duration tracker implementation
#[derive(Clone)]
pub struct OtelMethodDurationTracker {
    metrics: Arc<OtelMetrics>,
}

impl OtelMethodDurationTracker {
    pub fn new(metrics: Arc<OtelMetrics>) -> Self {
        Self { metrics }
    }
}

#[async_trait]
impl MethodDurationTracker for OtelMethodDurationTracker {
    async fn track_duration(
        &self,
        context: &RequestContext,
        user: Option<&AuthenticatedUser>,
        duration: Duration,
    ) {
        // Log includes user for debugging, but metrics don't to avoid cardinality issues
        let user_id = user.map(|u| u.user_id.as_str()).unwrap_or("anonymous");

        info!(
            protocol = %context.protocol,
            method = %context.method,
            user_id = %user_id,
            duration_ms = %duration.as_millis(),
            "Request completed"
        );

        self.metrics.record_method_duration(context, duration);
        self.metrics.increment_requests_completed(context, true);
    }
}

/// Builder for setting up OpenTelemetry with Prometheus
pub struct OtelSetupBuilder {
    service_name: &'static str,
    prometheus_registry: Option<Registry>,
}

impl OtelSetupBuilder {
    /// Create a new builder with the given service name
    pub fn new(service_name: &'static str) -> Self {
        Self {
            service_name,
            prometheus_registry: None,
        }
    }

    /// Use an existing Prometheus registry
    pub fn with_prometheus_registry(mut self, registry: Registry) -> Self {
        self.prometheus_registry = Some(registry);
        self
    }

    /// Build and initialize OpenTelemetry
    pub fn build(self) -> Result<OtelSetup, Box<dyn std::error::Error>> {
        // Create or use existing Prometheus registry
        let prometheus_registry = self.prometheus_registry.unwrap_or_default();

        // Create Prometheus exporter
        let prometheus_exporter = opentelemetry_prometheus::exporter()
            .with_registry(prometheus_registry.clone())
            .build()?;

        // Build meter provider
        let meter_provider = SdkMeterProvider::builder()
            .with_reader(prometheus_exporter)
            .build();

        // Set as global provider
        global::set_meter_provider(meter_provider.clone());

        // Create meter
        let meter = global::meter(self.service_name);

        // Create metrics
        let metrics = Arc::new(OtelMetrics::new(&meter));

        Ok(OtelSetup {
            meter_provider: Arc::new(meter_provider),
            prometheus_registry: Arc::new(prometheus_registry),
            metrics,
            service_name: self.service_name.to_string(),
        })
    }
}

/// Result of OpenTelemetry setup
pub struct OtelSetup {
    pub meter_provider: Arc<SdkMeterProvider>,
    pub prometheus_registry: Arc<Registry>,
    pub metrics: Arc<OtelMetrics>,
    pub service_name: String,
}

impl OtelSetup {
    /// Create a usage tracker
    pub fn usage_tracker(&self) -> OtelUsageTracker {
        OtelUsageTracker::new(self.metrics.clone())
    }

    /// Create a method duration tracker
    pub fn method_duration_tracker(&self) -> OtelMethodDurationTracker {
        OtelMethodDurationTracker::new(self.metrics.clone())
    }

    /// Force flush all pending metrics
    pub fn force_flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.meter_provider.force_flush()?;
        Ok(())
    }

    /// Create an Axum router for the metrics endpoint
    pub fn metrics_router(&self) -> Router {
        Router::new()
            .route("/metrics", get(metrics_handler))
            .with_state(self.prometheus_registry.clone())
    }

    /// Get the metrics instance for custom tracking
    pub fn metrics(&self) -> Arc<OtelMetrics> {
        self.metrics.clone()
    }
}

/// Handler for Prometheus metrics endpoint
async fn metrics_handler(
    State(prometheus_registry): State<Arc<Registry>>,
) -> Result<Response<Body>, StatusCode> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus_registry.gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(Body::from(buffer))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Convenience function to create a standard observability setup
pub fn standard_setup(service_name: &'static str) -> Result<OtelSetup, Box<dyn std::error::Error>> {
    OtelSetupBuilder::new(service_name).build()
}

#[cfg(test)]
mod tests;
