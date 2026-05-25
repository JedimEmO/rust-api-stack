//! Example showing how to integrate observability with REST service macros

use axum::Router;
use ras_observability_core::{MethodDurationTracker, RequestContext, UsageTracker};
use ras_observability_otel::OtelSetupBuilder;
use tracing::info;

/// Example of how service builders can integrate with observability
pub struct ServiceWithObservability {
    app: Router,
}

impl ServiceWithObservability {
    pub fn new() -> Self {
        // Set up observability
        let otel = OtelSetupBuilder::new("rest-service-with-observability")
            .build()
            .expect("Failed to set up OpenTelemetry");

        // Create usage tracker callback
        let _usage_tracker = {
            let usage_tracker = otel.usage_tracker();
            move |headers: axum::http::HeaderMap,
                  user: Option<ras_auth_core::AuthenticatedUser>,
                  method: &str,
                  path: &str| {
                let context = RequestContext::rest(method, path);
                let usage_tracker = usage_tracker.clone();

                async move {
                    usage_tracker
                        .track_request(&headers, user.as_ref(), &context)
                        .await;
                }
            }
        };

        // Create duration tracker callback
        let _duration_tracker = {
            let duration_tracker = otel.method_duration_tracker();
            move |method: &str,
                  path: &str,
                  user: Option<&ras_auth_core::AuthenticatedUser>,
                  duration: std::time::Duration| {
                let context = RequestContext::rest(method, path);
                let duration_tracker = duration_tracker.clone();
                let user_cloned = user.cloned();

                async move {
                    duration_tracker
                        .track_duration(&context, user_cloned.as_ref(), duration)
                        .await;
                }
            }
        };

        info!("Service configured with OpenTelemetry observability");

        // The example keeps the service route minimal while exposing the metrics endpoint.
        // A REST macro integration would normally assemble the application router.
        let app = Router::new()
            .route("/api/v1/health", axum::routing::get(|| async { "OK" }))
            .merge(otel.metrics_router());

        Self { app }
    }

    pub fn into_router(self) -> Router {
        self.app
    }
}

impl Default for ServiceWithObservability {
    fn default() -> Self {
        Self::new()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let service = ServiceWithObservability::new();
    let app = service.into_router();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;

    println!("REST service with observability running on http://localhost:3000");
    println!("Health check: http://localhost:3000/api/v1/health");
    println!("Metrics: http://localhost:3000/metrics");

    axum::serve(listener, app).await?;

    Ok(())
}
