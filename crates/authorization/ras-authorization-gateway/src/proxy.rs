//! The proxying layer: an axum service that validates, narrows, and
//! forwards.
//!
//! Header hygiene: inbound `Authorization`, `Cookie`, `Host`, and all
//! hop-by-hop headers are stripped before proxying; the only credential a
//! backend ever receives is the gateway-derived bearer. Request and response
//! bodies stream without buffering. Connection upgrades (WebSocket) fail
//! closed in v1.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::TryStreamExt;
use ras_transport_core::{
    HttpTransport, RequestBody, TransportError, TransportRequest, byte_stream_from,
};

use crate::error::GatewayError;
use crate::gateway::AuthGateway;

/// Hop-by-hop headers (RFC 9110 §7.6.1) that must not be proxied.
const HOP_BY_HOP: [HeaderName; 8] = [
    header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

struct ProxyState {
    gateway: Arc<AuthGateway>,
    upstream: Arc<dyn HttpTransport>,
}

/// Build the gateway as an axum [`Router`].
///
/// `upstream` executes the proxied requests: [`ras_transport_core::ReqwestTransport`]
/// in production, a fake or `AxumTestTransport` in tests.
pub fn gateway_router(gateway: Arc<AuthGateway>, upstream: Arc<dyn HttpTransport>) -> Router {
    Router::new()
        .fallback(proxy_handler)
        .with_state(Arc::new(ProxyState { gateway, upstream }))
}

fn error_response(err: &GatewayError) -> Response {
    let status = match err {
        GatewayError::RouteNotFound => StatusCode::NOT_FOUND,
        GatewayError::MissingSession | GatewayError::InvalidSession(_) => StatusCode::UNAUTHORIZED,
        GatewayError::NoPermissionsForAudience { .. } => StatusCode::FORBIDDEN,
        GatewayError::UpgradeNotSupported => StatusCode::NOT_IMPLEMENTED,
        GatewayError::Upstream(_) => StatusCode::BAD_GATEWAY,
        GatewayError::InvalidConfig(_) | GatewayError::Derivation(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    // Audit hook: outcomes are traced without token values.
    tracing::debug!(error = %err, status = %status, "gateway request rejected");
    status.into_response()
}

/// Extract the web session credential: `Authorization: Bearer` first, then
/// the configured session cookie.
fn extract_session_token(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    if let Some(value) = headers.get(header::AUTHORIZATION)
        && let Ok(value) = value.to_str()
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        return Some(token.to_string());
    }
    for value in headers.get_all(header::COOKIE) {
        let Ok(value) = value.to_str() else { continue };
        for piece in cookie::Cookie::split_parse(value).flatten() {
            if piece.name() == cookie_name {
                return Some(piece.value().to_string());
            }
        }
    }
    None
}

async fn proxy_handler(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    match proxy(state, request).await {
        Ok(response) => response,
        Err(err) => error_response(&err),
    }
}

async fn proxy(state: Arc<ProxyState>, request: Request) -> Result<Response, GatewayError> {
    // v1: connection upgrades (WebSocket) fail closed. Bidirectional RAS
    // services must be reached directly or through a future upgrade-aware
    // gateway version.
    if request.headers().contains_key(header::UPGRADE) {
        return Err(GatewayError::UpgradeNotSupported);
    }

    let route = state
        .gateway
        .routes()
        .match_path(request.uri().path())
        .ok_or(GatewayError::RouteNotFound)?
        .clone();

    let token = extract_session_token(request.headers(), state.gateway.session_cookie())
        .ok_or(GatewayError::MissingSession)?;
    let session = state.gateway.validate_session(&token)?;
    let derived = state.gateway.derive_for_route(&session, &route)?;

    // Build the outbound request: upstream base + original path and query,
    // no rewriting.
    let path_and_query = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{}{}", route.upstream.trim_end_matches('/'), path_and_query);

    let mut outbound = TransportRequest::new(request.method().clone(), url);
    for (name, value) in request.headers() {
        if HOP_BY_HOP.contains(name)
            || name == header::AUTHORIZATION
            || name == header::COOKIE
            || name == header::HOST
            || name == header::CONTENT_LENGTH
        {
            continue;
        }
        outbound.headers.append(name.clone(), value.clone());
    }
    // The derived bearer is the only credential the backend sees. Set
    // fail-closed: an unencodable token aborts rather than proxying
    // unauthenticated.
    let outbound = outbound
        .bearer(&derived.token)
        .map_err(GatewayError::Upstream)?;

    let body_stream = request
        .into_body()
        .into_data_stream()
        .map_err(|err| TransportError::Body(err.to_string()));
    let outbound = outbound.body(RequestBody::Stream(byte_stream_from(body_stream)));

    let upstream_response = state.upstream.execute(outbound).await?;

    let mut response = Response::builder().status(upstream_response.status());
    if let Some(headers) = response.headers_mut() {
        for (name, value) in upstream_response.headers() {
            if HOP_BY_HOP.contains(name) || name == header::CONTENT_LENGTH {
                continue;
            }
            headers.append(name.clone(), value.clone());
        }
    }
    let body = Body::from_stream(upstream_response.into_body_stream());
    response
        .body(body)
        .map_err(|err| GatewayError::InvalidConfig(format!("response build failed: {err}")))
}
