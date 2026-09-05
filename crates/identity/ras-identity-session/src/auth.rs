use crate::{SessionError, SessionService};
use async_trait::async_trait;
use ras_auth_core::{AuthError, AuthFuture, AuthProvider, AuthenticatedUser};
use std::sync::Arc;

#[derive(Clone)]
pub struct JwtAuthProvider {
    session_service: Arc<SessionService>,
}

impl JwtAuthProvider {
    pub fn new(session_service: Arc<SessionService>) -> Self {
        Self { session_service }
    }
}

#[async_trait]
impl AuthProvider for JwtAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let claims =
                self.session_service
                    .verify_session(&token)
                    .await
                    .map_err(|e| match e {
                        SessionError::TokenExpired => AuthError::TokenExpired,
                        _ => AuthError::InvalidToken,
                    })?;

            Ok(AuthenticatedUser {
                user_id: claims.sub,
                permissions: claims.permissions,
                metadata: claims.metadata,
            })
        })
    }
}
