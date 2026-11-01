use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// The provider-scoped user ID, `"<provider_id>:<subject>"`, built by
    /// [`crate::user_id`]. Key application data on this, never on the bare subject.
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    /// Not-before. Optional; when present the token is rejected until then
    /// (with [`crate::CLOCK_SKEW_LEEWAY_SECS`] leeway).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    pub jti: String,
    /// The provider ID from the verified identity, such as `local` or `oauth2:google`.
    pub provider_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub permissions: HashSet<String>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
}
