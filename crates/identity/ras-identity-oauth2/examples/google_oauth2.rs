//! Example showing how to set up Google OAuth2 authentication
//!
//! This example demonstrates:
//! 1. Setting up OAuth2 provider configuration
//! 2. Integrating with SessionService
//! 3. Handling the OAuth2 flow
//! 4. Issuing JWTs after successful authentication

use ras_identity_oauth2::{
    InMemoryStateStore, OAuth2Config, OAuth2Provider, OAuth2ProviderConfig, OAuth2Response,
};
use ras_identity_session::{SessionConfig, SessionService};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Google OAuth2 configuration
    let google_config = OAuth2ProviderConfig {
        provider_id: "google".to_string(),
        client_id: std::env::var("GOOGLE_CLIENT_ID")
            .expect("GOOGLE_CLIENT_ID must be set for the Google OAuth2 example"),
        client_secret: std::env::var("GOOGLE_CLIENT_SECRET")
            .expect("GOOGLE_CLIENT_SECRET must be set for the Google OAuth2 example"),
        authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
        token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
        userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v1/userinfo".to_string()),
        issuer: Some("https://accounts.google.com".to_string()),
        redirect_uri: "http://localhost:3000/auth/google/callback".to_string(),
        scopes: vec![
            "openid".to_string(),
            "email".to_string(),
            "profile".to_string(),
        ],
        auth_params: HashMap::new(),
        use_pkce: true,          // Enable PKCE for security
        user_info_mapping: None, // Use default mapping
    };

    // Create OAuth2 configuration
    let oauth2_config = OAuth2Config::new()
        .add_provider(google_config)
        .with_state_ttl(600) // 10 minutes
        .with_http_timeout(30); // 30 seconds

    // Create state store and OAuth2 provider. The provider is cheap to clone;
    // keep one handle for flow initiation and register the other for
    // verification through the session service.
    let state_store = Arc::new(InMemoryStateStore::new());
    let oauth2_provider = OAuth2Provider::new(oauth2_config, state_store);

    // Create session service
    let session_config =
        SessionConfig::new("oauth2-example-secret-that-is-long-enough-for-tests").unwrap();
    let session_service = SessionService::new(session_config).unwrap();

    // Register OAuth2 provider with session service
    session_service
        .register_provider(Box::new(oauth2_provider.clone()))
        .await;

    println!("OAuth2 Example - Google Authentication");
    println!("=====================================");

    // Step 1: Start OAuth2 flow
    println!("\n1. Starting OAuth2 flow...");

    match oauth2_provider.start_flow("google", None).await {
        Ok(OAuth2Response::AuthorizationUrl { url, state }) => {
            println!("Authorization URL: {}", url);
            println!("State: {}", state);
            println!("\nIn a real application, you would:");
            println!("1. Redirect the user to the authorization URL");
            println!("2. Handle the callback with the authorization code");
            println!("3. Exchange the code for a JWT token");

            // Simulate callback (in real app, this comes from OAuth2 provider)
            simulate_callback(&session_service, state).await?;
        }
        Ok(OAuth2Response::Error { message }) => {
            println!("OAuth2 error: {}", message);
        }
        Err(e) => {
            println!("Error starting OAuth2 flow: {}", e);
        }
    }

    Ok(())
}

async fn simulate_callback(
    session_service: &SessionService,
    state: String,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n2. Simulating OAuth2 callback...");

    // In a real application, these values would come from the OAuth2 provider callback
    let callback_payload = serde_json::json!({
        "type": "Callback",
        "provider_id": "google",
        "code": "simulated_authorization_code",
        "state": state
    });

    match session_service
        .begin_session("oauth2", callback_payload)
        .await
    {
        Ok(jwt_token) => {
            println!("OAuth2 authentication successful.");
            println!("JWT Token: {}", jwt_token);

            // Verify the token
            println!("\n3. Verifying JWT token...");
            match session_service.verify_session(&jwt_token).await {
                Ok(claims) => {
                    println!("Token verified successfully.");
                    println!("User ID: {}", claims.sub);
                    println!("Email: {:?}", claims.email);
                    println!("Display Name: {:?}", claims.display_name);
                    println!("Provider: {}", claims.provider_id);
                    println!("Permissions: {:?}", claims.permissions);
                }
                Err(e) => {
                    println!("Token verification failed: {}", e);
                }
            }
        }
        Err(e) => {
            println!("OAuth2 callback failed: {}", e);
            println!(
                "Note: This is expected in the simulation as we're not using real OAuth2 endpoints"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ras_identity_core::IdentityProvider;

    #[tokio::test]
    async fn test_oauth2_configuration() {
        let google_config = OAuth2ProviderConfig {
            provider_id: "google".to_string(),
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
            userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v1/userinfo".to_string()),
            issuer: Some("https://accounts.google.com".to_string()),
            redirect_uri: "http://localhost:3000/callback".to_string(),
            scopes: vec!["openid".to_string(), "email".to_string()],
            auth_params: HashMap::new(),
            use_pkce: true,
            user_info_mapping: None,
        };

        let config = OAuth2Config::new().add_provider(google_config);
        let state_store = Arc::new(InMemoryStateStore::new());
        let provider = OAuth2Provider::new(config, state_store);

        // Test provider creation
        assert_eq!(provider.provider_id(), "oauth2");
    }
}
