use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Router;
use axum_test::TestServer;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};

#[derive(Clone, Debug)]
pub struct MockAuthProvider {
    table: HashMap<String, AuthenticatedUser>,
}

impl Default for MockAuthProvider {
    fn default() -> Self {
        let mut table = HashMap::new();
        table.insert("user-token".to_string(), mock_user("user-1", &["user"]));
        table.insert(
            "admin-token".to_string(),
            mock_user("admin-1", &["admin", "user"]),
        );
        table.insert("readonly-token".to_string(), mock_user("ro-1", &["read"]));
        Self { table }
    }
}

impl AuthProvider for MockAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        let result = self
            .table
            .get(&token)
            .cloned()
            .ok_or(AuthError::InvalidToken);
        Box::pin(async move { result })
    }
}

pub fn mock_user(user_id: &str, permissions: &[&str]) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: user_id.to_string(),
        permissions: permissions
            .iter()
            .map(|p| (*p).to_string())
            .collect::<HashSet<_>>(),
        metadata: None,
    }
}

pub fn mock_http_server(router: Router) -> TestServer {
    TestServer::builder()
        .mock_transport()
        .build(router)
        .expect("failed to start axum-test TestServer with in-memory transport")
}

/// Build an in-memory `TestServer` wrapped in an `Arc` for sharing with an
/// [`AxumTestTransport`].
#[allow(dead_code)]
pub fn mock_http_server_arc(router: Router) -> Arc<TestServer> {
    Arc::new(mock_http_server(router))
}

/// Wrap a shared `TestServer` into an in-process [`HttpTransport`] suitable for
/// driving a generated client.
#[allow(dead_code)]
pub fn axum_transport(server: Arc<TestServer>) -> Arc<dyn ras_transport_core::HttpTransport> {
    Arc::new(ras_transport_core::AxumTestTransport::from_arc(server))
}
