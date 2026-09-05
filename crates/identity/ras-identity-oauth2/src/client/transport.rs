use crate::types::{TokenResponse, UserInfoResponse};
use crate::{OAuth2Error, OAuth2Result};
use reqwest::Client;
use std::{collections::HashMap, time::Duration};
use tracing::{debug, error, info, warn};

#[async_trait::async_trait]
pub(crate) trait OAuth2HttpTransport: Send + Sync {
    async fn exchange_code(
        &self,
        token_endpoint: &str,
        params: &HashMap<String, String>,
    ) -> OAuth2Result<TokenResponse>;

    async fn get_user_info(
        &self,
        userinfo_endpoint: &str,
        access_token: &str,
    ) -> OAuth2Result<UserInfoResponse>;
}

#[derive(Clone)]
pub(super) struct ReqwestOAuth2HttpTransport {
    client: Client,
}

#[async_trait::async_trait]
impl OAuth2HttpTransport for ReqwestOAuth2HttpTransport {
    async fn exchange_code(
        &self,
        token_endpoint: &str,
        params: &HashMap<String, String>,
    ) -> OAuth2Result<TokenResponse> {
        let response = self
            .client
            .post(token_endpoint)
            .form(params)
            .send()
            .await
            .map_err(log_upstream_error)?;

        if !response.status().is_success() {
            // Never log or propagate the raw provider response body — it can
            // contain tokens or other sensitive material. Status only.
            let status = response.status();
            error!("Token exchange failed with status {}", status);
            return Err(OAuth2Error::TokenExchangeFailed(format!(
                "token endpoint returned status {status}"
            )));
        }

        let token_response: TokenResponse = response.json().await.map_err(|e| {
            // reqwest decode errors embed the request URL; keep that in the log only (I6).
            warn!(error = %e, "token endpoint returned an undecodable response");
            OAuth2Error::InvalidTokenResponse("undecodable token response".to_string())
        })?;

        info!("Successfully exchanged code for tokens");
        Ok(token_response)
    }

    async fn get_user_info(
        &self,
        userinfo_endpoint: &str,
        access_token: &str,
    ) -> OAuth2Result<UserInfoResponse> {
        let response = self
            .client
            .get(userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(log_upstream_error)?;

        if !response.status().is_success() {
            // Status only; the raw body may echo the bearer token.
            let status = response.status();
            error!("User info request failed with status {}", status);
            return Err(OAuth2Error::UserInfoFailed(format!(
                "userinfo endpoint returned status {status}"
            )));
        }

        let user_info: UserInfoResponse = response.json().await.map_err(|e| {
            warn!(error = %e, "userinfo endpoint returned an undecodable response");
            OAuth2Error::InvalidUserInfoResponse("undecodable userinfo response".to_string())
        })?;

        debug!(
            "Successfully retrieved user info for subject: {}",
            user_info.sub
        );
        Ok(user_info)
    }
}

impl ReqwestOAuth2HttpTransport {
    pub(super) fn new(timeout_seconds: u64) -> OAuth2Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()?;
        Ok(Self { client })
    }
}

/// Log a transport-level failure at `warn` (the `reqwest::Error` carries the
/// request URL) and hand back the fixed-message error variant (I6).
fn log_upstream_error(error: reqwest::Error) -> OAuth2Error {
    warn!(error = %error, "upstream OAuth2 request failed");
    OAuth2Error::HttpError(error)
}
