use super::{OAuth2Client, PkceChallenge};
use crate::state::OAuth2State;
use crate::{OAuth2Error, OAuth2ProviderConfig, OAuth2Result};
use std::collections::HashMap;
use tracing::debug;
use url::Url;

/// Reserved OAuth/OIDC query parameters that the library sets itself. Neither
/// `provider_config.auth_params` nor caller-supplied `additional_params` may
/// override them — many providers honour the last occurrence of a duplicated
/// query parameter, so an injected second `redirect_uri` / `state` / PKCE value
/// would be an authorization-code-theft or CSRF vector.
const RESERVED_AUTH_PARAMS: &[&str] = &[
    "response_type",
    "client_id",
    "client_secret",
    "redirect_uri",
    "state",
    "nonce",
    "scope",
    "code_challenge",
    "code_challenge_method",
    "grant_type",
    "code",
    "code_verifier",
    // OIDC request objects (Core §6): parameters inside a `request` /
    // `request_uri` JWT take precedence over the query parameters we set, so
    // permitting them would re-establish the exact override primitive this
    // denylist removes (e.g. silently dropping PKCE or overriding state/nonce).
    "request",
    "request_uri",
    // Response delivery / audience controls a caller must not influence.
    "response_mode",
    "resource",
    "audience",
    "id_token_hint",
];

/// Reject any key that collides (case-insensitively) with a reserved parameter.
fn reject_reserved_params<'a, I>(keys: I, source: &str) -> OAuth2Result<()>
where
    I: IntoIterator<Item = &'a String>,
{
    for key in keys {
        if RESERVED_AUTH_PARAMS
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(key))
        {
            return Err(OAuth2Error::InvalidAuthorizationParam(format!(
                "{source} may not set the reserved parameter `{key}`"
            )));
        }
    }
    Ok(())
}

impl OAuth2Client {
    /// Generate authorization URL for a provider
    pub async fn generate_authorization_url(
        &self,
        provider_config: &OAuth2ProviderConfig,
        additional_params: HashMap<String, String>,
    ) -> OAuth2Result<(String, String)> {
        self.generate_authorization_url_bound(provider_config, additional_params, None)
            .await
    }

    /// Generate an authorization URL bound to the initiating browser session.
    ///
    /// `binding` should be an unguessable value the integrator can recover on
    /// callback (e.g. a random cookie value); the callback must then present
    /// the identical value, preventing login CSRF where an attacker tricks a
    /// victim into completing the attacker's flow.
    pub async fn generate_authorization_url_bound(
        &self,
        provider_config: &OAuth2ProviderConfig,
        additional_params: HashMap<String, String>,
        binding: Option<String>,
    ) -> OAuth2Result<(String, String)> {
        // Reject reserved-parameter overrides before doing any work.
        reject_reserved_params(provider_config.auth_params.keys(), "provider auth_params")?;
        reject_reserved_params(additional_params.keys(), "additional_params")?;

        let mut url = Url::parse(&provider_config.authorization_endpoint)?;

        // Generate PKCE if enabled
        let pkce = if provider_config.use_pkce {
            Some(PkceChallenge::new())
        } else {
            None
        };

        // OIDC nonce: echoed back inside the id_token and verified on
        // callback, binding the token to this authorization request.
        let nonce = uuid::Uuid::new_v4().to_string();

        // Create and store state
        let state = OAuth2State::new(
            provider_config.provider_id.clone(),
            provider_config.redirect_uri.clone(),
            pkce.as_ref().map(|p| p.code_verifier.clone()),
            self.state_ttl_seconds,
        )
        .with_nonce(nonce.clone())
        .with_binding(binding);

        let state_param = state.state.clone();
        self.state_store.store(state).await?;

        // Build query parameters
        let mut params = url.query_pairs_mut();
        params.append_pair("response_type", "code");
        params.append_pair("client_id", &provider_config.client_id);
        params.append_pair("redirect_uri", &provider_config.redirect_uri);
        params.append_pair("state", &state_param);
        params.append_pair("nonce", &nonce);

        // Add scopes
        if !provider_config.scopes.is_empty() {
            params.append_pair("scope", &provider_config.scopes.join(" "));
        }

        // Add PKCE parameters
        if let Some(pkce) = &pkce {
            params.append_pair("code_challenge", &pkce.code_challenge);
            params.append_pair("code_challenge_method", &pkce.code_challenge_method);
        }

        // Add provider-specific parameters
        for (key, value) in &provider_config.auth_params {
            params.append_pair(key, value);
        }

        // Add additional parameters from the request
        for (key, value) in &additional_params {
            params.append_pair(key, value);
        }

        drop(params);

        let auth_url = url.to_string();
        debug!(
            "Generated authorization URL for provider {}",
            provider_config.provider_id
        );

        Ok((auth_url, state_param))
    }
}
