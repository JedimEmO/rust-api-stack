//! OAuth2 protocol types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Placeholder printed by `Debug` in place of a credential.
pub(crate) const REDACTED: &str = "[REDACTED]";

/// Redact an optional secret, keeping whether it was present.
pub(crate) fn redact_option(value: &Option<String>) -> Option<&'static str> {
    value.as_ref().map(|_| REDACTED)
}

/// Request to initiate OAuth2 authorization flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub additional_params: HashMap<String, String>,
}

/// Response from OAuth2 authorization callback
///
/// `Debug` redacts `code` and `binding`.
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthorizationResponse {
    /// Authorization code; absent when the provider redirected back with an
    /// `error` (e.g. `access_denied`) instead.
    #[serde(default)]
    pub code: Option<String>,
    pub state: String,
    pub error: Option<String>,
    pub error_description: Option<String>,
    /// Session-binding value captured by the integrator when the flow was
    /// started (e.g. from a cookie). Must match the value given to
    /// `start_flow` for the same state, when one was supplied.
    #[serde(default)]
    pub binding: Option<String>,
}

impl fmt::Debug for AuthorizationResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationResponse")
            .field("code", &redact_option(&self.code))
            .field("state", &self.state)
            .field("error", &self.error)
            .field("error_description", &self.error_description)
            .field("binding", &redact_option(&self.binding))
            .finish()
    }
}

/// An authorization URL to send the browser to, from
/// [`OAuth2Client::generate_authorization_url`](crate::OAuth2Client::generate_authorization_url).
///
/// `Debug` redacts `binding`.
#[derive(Clone)]
pub struct AuthorizationRedirect {
    /// The provider's authorization URL with all query parameters set.
    pub url: String,
    /// The `state` parameter the callback will carry.
    pub state: String,
    /// The login-CSRF binding the callback must present. Store it in a cookie
    /// on the initiating browser. `None` only for a flow started with
    /// `generate_authorization_url_bound(.., None)`.
    pub binding: Option<String>,
}

impl fmt::Debug for AuthorizationRedirect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationRedirect")
            .field("url", &self.url)
            .field("state", &self.state)
            .field("binding", &redact_option(&self.binding))
            .finish()
    }
}

/// OAuth2 token response
///
/// `Debug` redacts `access_token`, `refresh_token` and `id_token`.
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub id_token: Option<String>,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &REDACTED)
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("refresh_token", &redact_option(&self.refresh_token))
            .field("scope", &self.scope)
            .field("id_token", &redact_option(&self.id_token))
            .finish()
    }
}

/// OAuth2 user info response (OpenID Connect compatible)
///
/// This struct supports both OpenID Connect standard field names and
/// legacy OAuth2 providers that use different field names. Specifically:
/// - `sub` (standard OpenID Connect) or `id` (Google OAuth2 v1) for the user identifier
/// - `email_verified` (OpenID Connect) for email verification status
///
/// The struct is designed to work with multiple Google OAuth2 userinfo endpoints:
/// - v1: `https://www.googleapis.com/oauth2/v1/userinfo` (returns `id`)
/// - v2: `https://www.googleapis.com/oauth2/v2/userinfo` (returns `sub`)
/// - v3: `https://www.googleapis.com/oauth2/v3/userinfo` (returns `sub`)
/// - OpenID Connect: `https://openidconnect.googleapis.com/v1/userinfo` (returns `sub`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfoResponse {
    /// User identifier - accepts both "sub" (OpenID Connect standard) and "id" (Google v1 API)
    #[serde(alias = "id")]
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
    pub locale: Option<String>,
    #[serde(flatten)]
    pub additional_claims: HashMap<String, serde_json::Value>,
}

/// OAuth2 provider metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: Option<String>,
    pub scopes_supported: Option<Vec<String>>,
    pub response_types_supported: Option<Vec<String>>,
    pub grant_types_supported: Option<Vec<String>>,
    pub code_challenge_methods_supported: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_info_response_deserialize_sub_field() {
        let json = r#"{
            "sub": "123456789",
            "email": "user@example.com",
            "email_verified": true,
            "name": "Test User"
        }"#;

        let user_info: UserInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(user_info.sub, "123456789");
        assert_eq!(user_info.email, Some("user@example.com".to_string()));
        assert_eq!(user_info.email_verified, Some(true));
        assert_eq!(user_info.name, Some("Test User".to_string()));
    }

    #[test]
    fn test_user_info_response_deserialize_id_field() {
        let json = r#"{
            "id": "123456789",
            "email": "user@example.com",
            "name": "Test User"
        }"#;

        let user_info: UserInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(user_info.sub, "123456789");
        assert_eq!(user_info.email, Some("user@example.com".to_string()));
        assert_eq!(user_info.name, Some("Test User".to_string()));
    }

    #[test]
    fn test_user_info_response_with_additional_claims() {
        let json = r#"{
            "id": "123456789",
            "email": "user@example.com",
            "name": "Test User",
            "custom_field": "custom_value",
            "another_field": 42
        }"#;

        let user_info: UserInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(user_info.sub, "123456789");
        assert_eq!(user_info.email, Some("user@example.com".to_string()));
        assert_eq!(user_info.name, Some("Test User".to_string()));

        // Verify additional claims are captured
        assert_eq!(
            user_info.additional_claims.get("custom_field").unwrap(),
            "custom_value"
        );
        assert_eq!(
            user_info.additional_claims.get("another_field").unwrap(),
            42
        );
    }
}
