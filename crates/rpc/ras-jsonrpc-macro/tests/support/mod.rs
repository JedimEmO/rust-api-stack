use std::collections::{HashMap, HashSet};

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

#[allow(dead_code)]
pub fn mock_http_server_arc(router: Router) -> std::sync::Arc<TestServer> {
    std::sync::Arc::new(mock_http_server(router))
}

#[allow(dead_code)]
pub fn axum_transport(
    server: std::sync::Arc<TestServer>,
) -> std::sync::Arc<dyn ras_transport_core::HttpTransport> {
    std::sync::Arc::new(ras_transport_core::AxumTestTransport::from_arc(server))
}
