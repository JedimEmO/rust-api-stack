//! Authentication, authorization, and HTTP credential handling for RAS services.

mod authorize;
mod transport;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use authorize::*;
pub use transport::*;

/// Maximum length of an attacker-influenced string included in a log line.
pub const MAX_LOG_DETAIL_BYTES: usize = 256;

/// Make an untrusted string safe to place in a log record.
///
/// Control characters (including newlines, which enable log injection) are
/// replaced with `?`, and the result is truncated to
/// [`MAX_LOG_DETAIL_BYTES`] on a UTF-8 boundary, the trailing `…` marker
/// included in that budget. Use it for
/// any request-derived detail, such as extractor rejection text, before it
/// reaches `tracing`.
pub fn sanitize_log_detail(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    const MARKER: char = '…';
    if out.len() > MAX_LOG_DETAIL_BYTES {
        let mut cut = MAX_LOG_DETAIL_BYTES - MARKER.len_utf8();
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push(MARKER);
    }
    out
}

/// Errors that can occur during authentication or authorization.
///
/// Deliberately **not** `Serialize`/`Deserialize`: this is a server-side
/// diagnostic type. [`AuthError::InsufficientPermissions`] carries the caller's
/// full permission set in `has`, which must never be sent over the wire. Map
/// to a generic per-class client message instead (as the generated servers do).
#[derive(Debug, Error, Clone)]
pub enum AuthError {
    /// The provided token is invalid or malformed.
    #[error("Invalid token")]
    InvalidToken,

    /// The token has expired.
    #[error("Token expired")]
    TokenExpired,

    /// The token does not have the required permissions.
    ///
    /// The `Display` output names the required set and only the *count* of
    /// permissions the caller holds; `has` is retained for server-side logging
    /// via `Debug`.
    #[error("Insufficient permissions: required {required:?}, caller holds {} permission(s)", has.len())]
    InsufficientPermissions {
        required: Vec<String>,
        has: Vec<String>,
    },

    /// Authentication is required but no token was provided.
    #[error("Authentication required")]
    AuthenticationRequired,

    /// An internal error occurred during authentication.
    #[error("Authentication error: {0}")]
    Internal(String),
}

/// Represents an authenticated user with their permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedUser {
    /// Unique identifier for the user.
    pub user_id: String,

    /// Set of permissions granted to this user.
    pub permissions: HashSet<String>,

    /// Optional additional metadata about the user.
    pub metadata: Option<serde_json::Value>,
}

/// The caller of an `OPTIONAL_AUTH` route.
///
/// An `OPTIONAL_AUTH` route is public — it is never rejected for authentication
/// reasons — but it opportunistically identifies its caller. The handler receives
/// this value as its first argument and decides how much to reveal:
///
/// * [`Caller::Anonymous`] — no credential, or a credential that failed to
///   authenticate (lenient: invalid/expired tokens and cookies that fail CSRF on
///   an unsafe method all resolve to anonymous).
/// * [`Caller::Authenticated`] — a valid credential was presented.
///
/// A `Caller` represents a resolved identity, so it cannot be deserialized from
/// request input. Construct it from trusted authentication results.
#[must_use]
#[derive(Debug, Clone)]
pub enum Caller {
    /// No authenticated caller — treat the request as public/anonymous.
    Anonymous,
    /// A caller whose credential authenticated successfully.
    Authenticated(AuthenticatedUser),
}

impl From<Option<AuthenticatedUser>> for Caller {
    /// Maps a best-effort authentication result to a caller: `Some(user)` ⇒
    /// [`Caller::Authenticated`], `None` ⇒ [`Caller::Anonymous`]. Used by the
    /// generated services that already hold an `Option<AuthenticatedUser>`.
    fn from(user: Option<AuthenticatedUser>) -> Self {
        match user {
            Some(user) => Caller::Authenticated(user),
            None => Caller::Anonymous,
        }
    }
}

impl Caller {
    /// Borrows the authenticated user, or `None` when anonymous.
    pub fn authenticated(&self) -> Option<&AuthenticatedUser> {
        match self {
            Caller::Authenticated(user) => Some(user),
            Caller::Anonymous => None,
        }
    }

    /// Returns `true` when a caller authenticated.
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Caller::Authenticated(_))
    }

    /// Consumes the caller, yielding the authenticated user when present.
    pub fn into_authenticated(self) -> Option<AuthenticatedUser> {
        match self {
            Caller::Authenticated(user) => Some(user),
            Caller::Anonymous => None,
        }
    }
}

/// Result type for authentication operations.
pub type AuthResult<T = AuthenticatedUser> = Result<T, AuthError>;

/// Boxed future for async authentication operations.
pub type AuthFuture<'a, T = AuthenticatedUser> =
    Pin<Box<dyn Future<Output = AuthResult<T>> + Send + 'a>>;

/// Trait for implementing authentication providers.
///
/// Services use this interface to validate tokens and check permissions.
pub trait AuthProvider: Send + Sync + 'static {
    /// Validates a token and returns the authenticated user.
    ///
    /// # Arguments
    /// * `token` - The authentication token to validate (e.g., JWT, API key)
    ///
    /// # Returns
    /// * `Ok(AuthenticatedUser)` if the token is valid
    /// * `Err(AuthError)` if validation fails
    fn authenticate(&self, token: String) -> AuthFuture<'_>;

    /// Checks if the authenticated user has the required permissions.
    ///
    /// # Arguments
    /// * `user` - The authenticated user
    /// * `required_permissions` - List of permissions that are required
    ///
    /// # Returns
    /// * `Ok(())` if the user has all required permissions
    /// * `Err(AuthError::InsufficientPermissions)` if any permission is missing
    fn check_permissions(
        &self,
        user: &AuthenticatedUser,
        required_permissions: &[String],
    ) -> AuthResult<()> {
        let missing_permissions: Vec<String> = required_permissions
            .iter()
            .filter(|perm| !user.permissions.contains(*perm))
            .cloned()
            .collect();

        if missing_permissions.is_empty() {
            Ok(())
        } else {
            Err(AuthError::InsufficientPermissions {
                required: required_permissions.to_vec(),
                has: user.permissions.iter().cloned().collect(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::task::{Context, Poll, Waker};

    use serde_json::json;

    use super::*;

    struct TestAuthProvider;

    impl AuthProvider for TestAuthProvider {
        fn authenticate(&self, _token: String) -> AuthFuture<'_> {
            unreachable!("permission tests only exercise the default helper")
        }
    }

    struct TokenAuthProvider;

    impl AuthProvider for TokenAuthProvider {
        fn authenticate(&self, token: String) -> AuthFuture<'_> {
            Box::pin(async move {
                if token != "good-token" {
                    return Err(AuthError::InvalidToken);
                }

                Ok(AuthenticatedUser {
                    user_id: "user-1".to_string(),
                    permissions: HashSet::from(["widgets:read".to_string()]),
                    metadata: Some(json!({ "tenant": "acme" })),
                })
            })
        }
    }

    fn poll_auth_future(mut future: AuthFuture<'_>) -> AuthResult {
        let waker = Waker::noop().clone();
        let mut context = Context::from_waker(&waker);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("test auth future should complete immediately"),
        }
    }

    fn user_with_permissions(permissions: &[&str]) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_string(),
            permissions: permissions
                .iter()
                .map(|permission| permission.to_string())
                .collect(),
            metadata: Some(json!({ "tenant": "acme" })),
        }
    }

    #[test]
    fn log_detail_strips_control_chars_and_truncates() {
        let injected = "bad value\n[ERROR] forged line\r\x1b[31m";
        let cleaned = sanitize_log_detail(injected);
        assert!(!cleaned.contains('\n') && !cleaned.contains('\r') && !cleaned.contains('\x1b'));
        assert!(cleaned.starts_with("bad value?[ERROR] forged line"));

        let long = "é".repeat(MAX_LOG_DETAIL_BYTES);
        let cut = sanitize_log_detail(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.len() <= MAX_LOG_DETAIL_BYTES);
    }

    #[test]
    fn check_permissions_allows_user_when_all_required_permissions_are_present() {
        let provider = TestAuthProvider;
        let user = user_with_permissions(&["users:read", "users:write"]);
        let required = vec!["users:read".to_string(), "users:write".to_string()];

        let result = provider.check_permissions(&user, &required);

        assert!(result.is_ok());
    }

    #[test]
    fn check_permissions_allows_user_when_no_permissions_are_required() {
        let provider = TestAuthProvider;
        let user = user_with_permissions(&[]);

        let result = provider.check_permissions(&user, &[]);

        assert!(result.is_ok());
    }

    #[test]
    fn check_permissions_returns_required_and_actual_permissions_when_one_is_missing() {
        let provider = TestAuthProvider;
        let user = user_with_permissions(&["users:read"]);
        let required = vec!["users:read".to_string(), "users:write".to_string()];

        let result = provider.check_permissions(&user, &required);

        let AuthError::InsufficientPermissions { required, has } = result.unwrap_err() else {
            panic!("expected insufficient permissions");
        };
        assert_eq!(required, vec!["users:read", "users:write"]);
        assert_eq!(
            has.into_iter().collect::<HashSet<_>>(),
            HashSet::from(["users:read".to_string()])
        );
    }

    #[test]
    fn authenticated_user_serializes_permissions_and_metadata() {
        let user = user_with_permissions(&["users:read", "users:write"]);

        let json = serde_json::to_value(&user).expect("serialize user");
        let round_trip: AuthenticatedUser = serde_json::from_value(json).expect("deserialize user");

        assert_eq!(round_trip.user_id, "user-1");
        assert_eq!(round_trip.permissions, user.permissions);
        assert_eq!(round_trip.metadata, Some(json!({ "tenant": "acme" })));
    }

    #[test]
    fn auth_error_display_messages_are_stable_for_clients() {
        assert_eq!(AuthError::InvalidToken.to_string(), "Invalid token");
        assert_eq!(AuthError::TokenExpired.to_string(), "Token expired");
        assert_eq!(
            AuthError::AuthenticationRequired.to_string(),
            "Authentication required"
        );
        assert_eq!(
            AuthError::Internal("store unavailable".to_string()).to_string(),
            "Authentication error: store unavailable"
        );
    }

    #[test]
    fn auth_provider_future_alias_returns_authenticated_user() {
        let provider = TokenAuthProvider;

        let user = poll_auth_future(provider.authenticate("good-token".to_string()))
            .expect("token authenticates");

        assert_eq!(user.user_id, "user-1");
        assert!(user.permissions.contains("widgets:read"));
        assert_eq!(user.metadata, Some(json!({ "tenant": "acme" })));

        assert!(poll_auth_future(provider.authenticate("bad-token".to_string())).is_err());
    }

    #[test]
    fn a2_insufficient_permissions_display_does_not_leak_held_permissions() {
        let error = AuthError::InsufficientPermissions {
            required: vec!["admin".to_string()],
            has: vec!["user".to_string(), "billing:read".to_string()],
        };

        let display = error.to_string();
        assert!(display.contains("required [\"admin\"]"), "{display}");
        assert!(display.contains("holds 2 permission(s)"), "{display}");
        assert!(!display.contains("user"), "{display}");
        assert!(!display.contains("billing:read"), "{display}");

        // `has` is kept for server-side logging through `Debug`.
        let debug = format!("{error:?}");
        assert!(debug.contains("billing:read"), "{debug}");
    }

    /// `AuthError` must not implement `Serialize` so it can never be emitted
    /// on the wire by accident. Compile-time check via autoref specialization:
    /// the `IsSerialize` impl on `Probe<T>` wins when `T: Serialize`; otherwise
    /// method lookup falls back to the `NotSerialize` impl on `&Probe<T>`.
    #[test]
    fn a2_auth_error_is_not_serializable() {
        struct Probe<T>(std::marker::PhantomData<T>);
        trait NotSerialize {
            fn is_serialize(&self) -> bool {
                false
            }
        }
        impl<T> NotSerialize for &Probe<T> {}
        trait IsSerialize {
            fn is_serialize(&self) -> bool {
                true
            }
        }
        impl<T: Serialize> IsSerialize for Probe<T> {}

        let auth_error = &Probe::<AuthError>(std::marker::PhantomData);
        assert!(!auth_error.is_serialize());
        // Sanity check that the probe detects a serializable type.
        let user = &Probe::<AuthenticatedUser>(std::marker::PhantomData);
        assert!(user.is_serialize());
    }
}
