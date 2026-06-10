//! Authorization control-plane errors.

use thiserror::Error;

/// Errors from the authorization store, identity verification, and token
/// issuance. All issuance paths fail closed; variants never carry secret or
/// token material.
#[derive(Debug, Error)]
pub enum AuthzError {
    /// Service identity proof did not verify. Deliberately carries no
    /// detail about why.
    #[error("service identity verification failed for {service_id:?}")]
    IdentityVerificationFailed { service_id: String },

    /// No service is registered under this id.
    #[error("unknown service {service_id:?}")]
    UnknownService { service_id: String },

    /// The service exists but is disabled.
    #[error("service {service_id:?} is disabled")]
    ServiceDisabled { service_id: String },

    /// No registered service owns the requested audience.
    #[error("unknown audience {audience:?}")]
    UnknownAudience { audience: String },

    /// The principal lacks one or more requested permissions for the
    /// target audience.
    #[error("permissions not granted for audience {audience:?}: {missing:?}")]
    PermissionsNotGranted {
        audience: String,
        missing: Vec<String>,
    },

    /// A loaded topology policy does not declare this caller→audience edge.
    #[error("service graph policy does not allow {caller:?} -> {audience:?}")]
    EdgeNotAllowed { caller: String, audience: String },

    /// A grant referenced a permission unknown to the target audience's
    /// imported manifests (and was not explicitly marked custom).
    #[error("permission {permission:?} is not a known permission of audience {audience:?}")]
    UnknownPermission {
        audience: String,
        permission: String,
    },

    /// Underlying token signing/validation failure.
    #[error("token error: {0}")]
    Token(#[from] ras_authorization_token::TokenError),

    /// Storage failure.
    #[error("authorization store error: {0}")]
    Store(String),

    /// Invalid configuration or registration input.
    #[error("invalid authorization configuration: {0}")]
    InvalidConfig(String),
}
