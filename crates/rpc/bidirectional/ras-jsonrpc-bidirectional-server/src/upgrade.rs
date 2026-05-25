//! WebSocket upgrade handling with authentication

use crate::{ServerError, ServerResult};
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade as AxumWebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use ras_auth_core::{AuthProvider, AuthenticatedUser};
use tracing::{debug, error, info, warn};

/// WebSocket upgrade handler with authentication support
pub struct WebSocketUpgrade {
    /// The underlying Axum WebSocket upgrade
    upgrade: AxumWebSocketUpgrade,
    /// Request headers for authentication
    headers: HeaderMap,
}

impl WebSocketUpgrade {
    /// Create a new WebSocket upgrade from Axum extractor
    pub fn new(upgrade: AxumWebSocketUpgrade, headers: HeaderMap) -> Self {
        Self { upgrade, headers }
    }

    /// Extract authentication token from headers
    pub fn extract_auth_token(&self) -> Option<String> {
        extract_auth_token_from_headers(&self.headers)
    }

    /// Authenticate the connection using the provided auth provider
    pub async fn authenticate<A: AuthProvider>(
        &self,
        auth_provider: &A,
    ) -> ServerResult<Option<AuthenticatedUser>> {
        authenticate_headers(&self.headers, auth_provider).await
    }

    /// Complete the WebSocket upgrade
    pub fn on_upgrade<F>(self, callback: F) -> Response
    where
        F: FnOnce(WebSocket) -> futures::future::BoxFuture<'static, ()> + Send + 'static,
    {
        self.upgrade.on_upgrade(callback)
    }

    /// Complete the WebSocket upgrade with authentication
    pub async fn on_upgrade_with_auth<A, F>(
        self,
        auth_provider: &A,
        require_auth: bool,
        callback: F,
    ) -> Result<Response, (StatusCode, String)>
    where
        A: AuthProvider,
        F: FnOnce(WebSocket, Option<AuthenticatedUser>) -> futures::future::BoxFuture<'static, ()>
            + Send
            + 'static,
    {
        // Authenticate before upgrading
        let auth_result = self.authenticate(auth_provider).await;

        match auth_result {
            Ok(user) => {
                // Check if authentication is required
                if require_auth && user.is_none() {
                    error!("Authentication required but no valid token provided");
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        "Authentication required".to_string(),
                    ));
                }

                // Complete the upgrade
                let response = self.upgrade.on_upgrade(move |socket| {
                    Box::pin(async move {
                        callback(socket, user).await;
                    })
                });

                Ok(response)
            }
            Err(e) => {
                error!("Authentication failed during WebSocket upgrade: {}", e);
                Err((e.to_status_code(), e.to_string()))
            }
        }
    }

    /// Get the underlying headers
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Check if a specific header is present
    pub fn has_header(&self, name: &str) -> bool {
        self.headers.contains_key(name)
    }

    /// Get a header value as string
    pub fn get_header(&self, name: &str) -> Option<String> {
        get_header_value(&self.headers, name)
    }

    /// Extract client IP from headers (useful for logging/security)
    pub fn extract_client_ip(&self) -> Option<String> {
        extract_client_ip_from_headers(&self.headers)
    }

    /// Extract user agent
    pub fn extract_user_agent(&self) -> Option<String> {
        self.get_header("user-agent")
    }

    /// Create connection metadata from headers
    pub fn create_metadata(&self) -> serde_json::Value {
        create_metadata_from_headers(&self.headers)
    }
}

fn extract_auth_token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers.get("authorization")
        && let Ok(auth_str) = auth_header.to_str()
    {
        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            return Some(token.to_string());
        }
        return Some(auth_str.to_string());
    }

    if let Some(token_header) = headers.get("sec-websocket-protocol")
        && let Ok(token_str) = token_header.to_str()
        && let Some(token) = token_str.strip_prefix("token.")
    {
        return Some(token.to_string());
    }

    if let Some(token_header) = headers.get("x-auth-token")
        && let Ok(token_str) = token_header.to_str()
    {
        return Some(token_str.to_string());
    }

    None
}

async fn authenticate_headers<A: AuthProvider>(
    headers: &HeaderMap,
    auth_provider: &A,
) -> ServerResult<Option<AuthenticatedUser>> {
    if let Some(token) = extract_auth_token_from_headers(headers) {
        debug!("Attempting to authenticate WebSocket connection");
        match auth_provider.authenticate(token).await {
            Ok(user) => {
                info!(
                    "WebSocket connection authenticated for user: {}",
                    user.user_id
                );
                Ok(Some(user))
            }
            Err(e) => {
                warn!("WebSocket authentication failed: {}", e);
                Err(ServerError::AuthenticationFailed(e))
            }
        }
    } else {
        debug!("No authentication token found in WebSocket headers");
        Ok(None)
    }
}

fn get_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn extract_client_ip_from_headers(headers: &HeaderMap) -> Option<String> {
    let ip_headers = [
        "x-forwarded-for",
        "x-real-ip",
        "cf-connecting-ip",
        "x-client-ip",
        "x-forwarded",
        "forwarded-for",
        "forwarded",
    ];

    for header_name in &ip_headers {
        if let Some(value) = get_header_value(headers, header_name) {
            let ip = value.split(',').next().unwrap_or(value.as_str()).trim();
            if !ip.is_empty() {
                return Some(ip.to_string());
            }
        }
    }

    None
}

fn create_metadata_from_headers(headers: &HeaderMap) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();

    if let Some(ip) = extract_client_ip_from_headers(headers) {
        metadata.insert("client_ip".to_string(), serde_json::Value::String(ip));
    }

    if let Some(user_agent) = get_header_value(headers, "user-agent") {
        metadata.insert(
            "user_agent".to_string(),
            serde_json::Value::String(user_agent),
        );
    }

    metadata.insert(
        "connected_at".to_string(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );

    serde_json::Value::Object(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, sync::Mutex};

    use axum::http::HeaderValue;
    use ras_auth_core::{AuthError, AuthFuture};

    struct RecordingAuthProvider {
        result: AuthResult,
        tokens: Mutex<Vec<String>>,
    }

    type AuthResult = Result<AuthenticatedUser, AuthError>;

    impl RecordingAuthProvider {
        fn returning(result: AuthResult) -> Self {
            Self {
                result,
                tokens: Mutex::new(Vec::new()),
            }
        }

        fn tokens(&self) -> Vec<String> {
            self.tokens.lock().expect("tokens lock").clone()
        }
    }

    impl AuthProvider for RecordingAuthProvider {
        fn authenticate(&self, token: String) -> AuthFuture<'_> {
            self.tokens.lock().expect("tokens lock").push(token);
            let result = self.result.clone();
            Box::pin(async move { result })
        }
    }

    fn test_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_string(),
            permissions: HashSet::from(["chat:read".to_string()]),
            metadata: None,
        }
    }

    #[test]
    fn extracts_authorization_bearer_token_first() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer abc123"));
        headers.insert("x-auth-token", HeaderValue::from_static("fallback"));

        assert_eq!(
            extract_auth_token_from_headers(&headers),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn extracts_authorization_raw_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("raw-token"));

        assert_eq!(
            extract_auth_token_from_headers(&headers),
            Some("raw-token".to_string())
        );
    }

    #[test]
    fn extracts_websocket_protocol_token_before_x_auth_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "sec-websocket-protocol",
            HeaderValue::from_static("token.ws-token"),
        );
        headers.insert("x-auth-token", HeaderValue::from_static("fallback"));

        assert_eq!(
            extract_auth_token_from_headers(&headers),
            Some("ws-token".to_string())
        );
    }

    #[test]
    fn extracts_x_auth_token_when_other_headers_are_absent() {
        let mut headers = HeaderMap::new();
        headers.insert("x-auth-token", HeaderValue::from_static("x-token"));

        assert_eq!(
            extract_auth_token_from_headers(&headers),
            Some("x-token".to_string())
        );
    }

    #[test]
    fn ignores_websocket_protocol_values_without_token_prefix() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "sec-websocket-protocol",
            HeaderValue::from_static("jsonrpc.v1"),
        );

        assert_eq!(extract_auth_token_from_headers(&headers), None);
    }

    #[test]
    fn extracts_first_forwarded_client_ip_and_trims_whitespace() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static(" 192.168.1.1, 10.0.0.1"),
        );

        assert_eq!(
            extract_client_ip_from_headers(&headers),
            Some("192.168.1.1".to_string())
        );
    }

    #[test]
    fn falls_back_to_next_ip_header_when_first_candidate_is_empty() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static(" , 10.0.0.1"));
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.7"));

        assert_eq!(
            extract_client_ip_from_headers(&headers),
            Some("203.0.113.7".to_string())
        );
    }

    #[test]
    fn returns_no_client_ip_when_known_headers_are_missing() {
        assert_eq!(extract_client_ip_from_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn creates_metadata_from_available_headers_and_timestamp() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("127.0.0.1"));
        headers.insert("user-agent", HeaderValue::from_static("test-agent"));

        let metadata = create_metadata_from_headers(&headers);

        assert_eq!(metadata.get("client_ip").expect("client ip"), "127.0.0.1");
        assert_eq!(
            metadata.get("user_agent").expect("user agent"),
            "test-agent"
        );
        let connected_at = metadata
            .get("connected_at")
            .and_then(serde_json::Value::as_str)
            .expect("connected_at timestamp");
        chrono::DateTime::parse_from_rfc3339(connected_at).expect("valid timestamp");
    }

    #[tokio::test]
    async fn authenticate_headers_returns_user_and_records_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
        let provider = RecordingAuthProvider::returning(Ok(test_user()));

        let user = authenticate_headers(&headers, &provider)
            .await
            .expect("authentication succeeds")
            .expect("authenticated user");

        assert_eq!(user.user_id, "user-1");
        assert_eq!(provider.tokens(), vec!["secret".to_string()]);
    }

    #[tokio::test]
    async fn authenticate_headers_returns_none_without_token() {
        let provider = RecordingAuthProvider::returning(Ok(test_user()));

        let user = authenticate_headers(&HeaderMap::new(), &provider)
            .await
            .expect("missing token is allowed");

        assert!(user.is_none());
        assert!(provider.tokens().is_empty());
    }

    #[tokio::test]
    async fn authenticate_headers_wraps_provider_errors() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("expired"));
        let provider = RecordingAuthProvider::returning(Err(AuthError::TokenExpired));

        let error = authenticate_headers(&headers, &provider)
            .await
            .expect_err("auth failure is propagated");

        assert_eq!(error.to_status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(error.to_string(), "Authentication failed: Token expired");
        assert_eq!(provider.tokens(), vec!["expired".to_string()]);
    }
}
