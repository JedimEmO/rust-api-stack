# ras-identity-oauth2

OAuth2 identity provider implementation with PKCE support for Rust Agent Stack.

## Features

- **OAuth2 Authorization Code Flow** with PKCE (Proof Key for Code Exchange) 
- **CSRF Protection** via state parameters
- **Generic OAuth2 Provider Support** (Google, GitHub, Microsoft, etc.)
- **Security-Focused Tests** for PKCE, state handling, and OAuth2 error paths
- **Thread-Safe** state management
- **Configurable User Info Mapping** for different provider schemas
- **Integration** with existing `IdentityProvider` trait

## Security Features

- **PKCE Support**: Mitigates authorization code interception attacks
- **State Parameter**: CSRF protection using cryptographically random UUIDs
- **OIDC Nonce**: Sent on every authorization request and verified against the id_token
- **id_token Claim Validation**: `iss` (when `issuer` is configured), `aud`, `exp` and `nonce` are checked on callback. The signature is not verified because the token arrives directly from the token endpoint over TLS, which OIDC Core §3.1.3.7 permits for the code flow
- **Session Binding (login-CSRF guard)**: `start_flow_bound` accepts an unguessable per-browser-session value (e.g. a random cookie); the callback payload must carry the identical `binding` or it is rejected, so an attacker cannot trick a victim into completing the attacker's flow
- **Input Validation**: Robust handling of malformed responses
- **Single-Use State**: Callback state is removed after successful retrieval

## Usage

### Basic Setup

```rust
use ras_identity_oauth2::{
    InMemoryStateStore, OAuth2Config, OAuth2Provider, OAuth2ProviderConfig, UserInfoMapping,
};
use std::{collections::HashMap, env, sync::Arc};

// Configure OAuth2 provider (e.g., Google)
let google_config = OAuth2ProviderConfig {
    provider_id: "google".to_string(),
    client_id: env::var("GOOGLE_CLIENT_ID")?,
    client_secret: env::var("GOOGLE_CLIENT_SECRET")?,
    authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
    token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
    userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v1/userinfo".to_string()),
    redirect_uri: "http://localhost:3000/auth/callback".to_string(),
    scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
    auth_params: HashMap::new(),
    use_pkce: true,
    user_info_mapping: None,
};

// Create OAuth2 configuration
let config = OAuth2Config::new()
    .add_provider(google_config)
    .with_state_ttl(600) // 10 minutes
    .with_http_timeout(30); // 30 seconds

// Create state store and provider
let state_store = Arc::new(InMemoryStateStore::new());
let oauth2_provider = OAuth2Provider::new(config, state_store);
```

### Integration with Session Service

```rust
use ras_identity_core::IdentityProvider;
use ras_identity_oauth2::OAuth2Response;
use ras_identity_session::{SessionConfig, SessionService};

// Register with session service. The provider is cheap to clone; keep one
// handle for flow initiation and register the other for verification.
let session_config = SessionConfig::new("use-at-least-32-bytes-of-random-secret")?;
let session_service = SessionService::new(session_config)?;

session_service.register_provider(Box::new(oauth2_provider.clone())).await;

// Start OAuth2 flow
match oauth2_provider.start_flow("google", None).await? {
    OAuth2Response::AuthorizationUrl { url, state } => {
        // Redirect user to `url`
        println!("Redirect to: {}", url);
    }
    OAuth2Response::Error { message } => {
        eprintln!("OAuth2 start-flow failed: {message}");
    }
}

// Handle callback
let callback_payload = serde_json::json!({
    "type": "Callback",
    "provider_id": "google",
    "code": "authorization_code_from_callback",
    "state": "state_from_callback"
});

let jwt_token = session_service.begin_session("oauth2", callback_payload).await?;
```

## OAuth2 Flow

1. **Start Flow**: Client requests authorization URL
2. **Redirect**: User is redirected to OAuth2 provider
3. **Authorization**: User grants permissions
4. **Callback**: Provider redirects back with authorization code
5. **Token Exchange**: Server exchanges code for access token using PKCE
6. **User Info**: Server fetches user information
7. **JWT Issuance**: Server issues JWT via SessionService

## Configuration Options

### OAuth2ProviderConfig

- `provider_id`: Unique identifier for the provider
- `client_id`: OAuth2 client ID
- `client_secret`: OAuth2 client secret
- `authorization_endpoint`: Provider's authorization URL
- `token_endpoint`: Provider's token exchange URL
- `userinfo_endpoint`: Provider's user info URL (optional)
- `issuer`: Expected `iss` claim of id_tokens (e.g. `https://accounts.google.com`); when set, id_tokens with a different issuer are rejected
- `redirect_uri`: Your application's callback URL
- `scopes`: Requested OAuth2 scopes
- `auth_params`: Additional authorization parameters
- `use_pkce`: Enable PKCE for enhanced security
- `user_info_mapping`: Custom field mapping for user info

### OAuth2Config

- `providers`: Map of provider configurations
- `state_ttl_seconds`: State parameter expiration time
- `http_timeout_seconds`: HTTP request timeout

## Provider Examples

### Google OAuth2

```rust
OAuth2ProviderConfig {
    provider_id: "google".to_string(),
    client_id: env::var("GOOGLE_CLIENT_ID")?,
    client_secret: env::var("GOOGLE_CLIENT_SECRET")?,
    authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
    token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
    userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v1/userinfo".to_string()),
    redirect_uri: "http://localhost:3000/auth/google/callback".to_string(),
    scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
    auth_params: HashMap::new(),
    use_pkce: true,
    user_info_mapping: None,
}
```

### GitHub OAuth2

```rust
OAuth2ProviderConfig {
    provider_id: "github".to_string(),
    client_id: env::var("GITHUB_CLIENT_ID")?,
    client_secret: env::var("GITHUB_CLIENT_SECRET")?,
    authorization_endpoint: "https://github.com/login/oauth/authorize".to_string(),
    token_endpoint: "https://github.com/login/oauth/access_token".to_string(),
    userinfo_endpoint: Some("https://api.github.com/user".to_string()),
    redirect_uri: "http://localhost:3000/auth/github/callback".to_string(),
    scopes: vec!["user:email".to_string()],
    auth_params: HashMap::new(),
    use_pkce: false, // Set according to provider support and client type.
    user_info_mapping: Some(UserInfoMapping {
        subject_field: Some("id".to_string()),
        email_field: Some("email".to_string()),
        name_field: Some("name".to_string()),
        picture_field: Some("avatar_url".to_string()),
    }),
}
```

## Custom State Storage

For production use, implement a custom state store:

```rust
use ras_identity_oauth2::{OAuth2Error, OAuth2Result, OAuth2State, OAuth2StateStore};
use async_trait::async_trait;

pub struct RedisStateStore {
    // Redis client implementation
}

impl RedisStateStore {
    async fn pop_state(&self, _state: &str) -> OAuth2Result<OAuth2State> {
        // Retrieve and delete state from Redis with your Redis client.
        Err(OAuth2Error::StateNotFound)
    }
}

#[async_trait]
impl OAuth2StateStore for RedisStateStore {
    async fn store(&self, state: OAuth2State) -> OAuth2Result<()> {
        // Store state in Redis with TTL
        Ok(())
    }

    async fn retrieve(&self, state: &str) -> OAuth2Result<OAuth2State> {
        self.pop_state(state).await
    }

    async fn cleanup_expired(&self) -> OAuth2Result<usize> {
        // Redis TTL handles expiration automatically
        Ok(0)
    }
}
```

## Security Considerations

- **Always use HTTPS** in production
- **Set appropriate state TTL** (5-10 minutes recommended)
- **Validate redirect URIs** match exactly
- **Use PKCE** when supported by the provider
- **Implement rate limiting** on OAuth2 endpoints
- **Monitor for state parameter attacks**
- **Keep client secrets secure** and rotate regularly

## Testing

The crate includes tests covering:

- PKCE generation and validation
- State parameter security
- Concurrent request handling
- Error cases and edge conditions
- Full OAuth2 flow simulation
- Callback state reuse and expiration scenarios

Run tests with:

```bash
cargo test -p ras-identity-oauth2 --locked
```

## Checks

```bash
cargo test -p ras-identity-oauth2 --locked
cargo clippy -p ras-identity-oauth2 --all-targets --all-features --locked -- -D warnings
```

## Dependencies

- `reqwest`: HTTP client for OAuth2 requests
- `serde`: Serialization for OAuth2 types
- `uuid`: Cryptographically random state generation
- `sha2`: SHA256 hashing for PKCE
- `base64`: URL-safe encoding
- `chrono`: Time handling for state expiration
