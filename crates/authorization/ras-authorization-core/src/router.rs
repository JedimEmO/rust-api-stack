//! Embedded-mode authority routes.
//!
//! [`authority_router`] mounts the token-issuance and JWKS endpoints in any
//! axum application, so a single process can be its own authority (the
//! default RAS deployment preset). Central-authority deployments serve the
//! same router from a dedicated service.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::AuthzError;
use crate::issuer::{InternalTokenRequest, TokenIssuer};

/// Response body for a successful token issuance.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(serde::Serialize)]
struct ErrorResponse {
    error: &'static str,
}

fn error_response(err: &AuthzError) -> Response {
    // Coarse error codes only: issuance callers learn *that* they were
    // denied, not the authorization topology behind the decision.
    let (status, code) = match err {
        AuthzError::IdentityVerificationFailed { .. } => {
            (StatusCode::UNAUTHORIZED, "identity_verification_failed")
        }
        AuthzError::UnknownService { .. }
        | AuthzError::ServiceDisabled { .. }
        | AuthzError::UnknownAudience { .. }
        | AuthzError::PermissionsNotGranted { .. }
        | AuthzError::EdgeNotAllowed { .. }
        | AuthzError::UnknownPermission { .. } => (StatusCode::FORBIDDEN, "issuance_denied"),
        AuthzError::Token(_) | AuthzError::Store(_) | AuthzError::InvalidConfig(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    };
    (status, Json(ErrorResponse { error: code })).into_response()
}

async fn issue_token(
    State(issuer): State<Arc<TokenIssuer>>,
    Json(request): Json<InternalTokenRequest>,
) -> Response {
    match issuer.issue_internal_token(request).await {
        Ok(issued) => Json(TokenResponse {
            token: issued.token,
            expires_at: issued.expires_at,
        })
        .into_response(),
        Err(err) => error_response(&err),
    }
}

async fn jwks(State(issuer): State<Arc<TokenIssuer>>) -> Response {
    Json(issuer.jwks().await).into_response()
}

/// Build the authority router:
///
/// - `POST /auth/token` — internal service token issuance
///   ([`InternalTokenRequest`] → [`TokenResponse`])
/// - `GET /auth/jwks.json` — public JWKS for downstream validation
///
/// Merge into an existing app for embedded mode, or serve standalone for a
/// central authority.
pub fn authority_router(issuer: Arc<TokenIssuer>) -> Router {
    Router::new()
        .route("/auth/token", post(issue_token))
        .route("/auth/jwks.json", get(jwks))
        .with_state(issuer)
}
