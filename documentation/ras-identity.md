# RAS Identity System Usage Guide

This guide covers the common setup for adding authentication and authorization to a RAS stack application using the identity crates.

## Overview

The RAS identity system provides a flexible, secure authentication framework with:
- Multiple authentication providers (local users, OAuth2)
- JWT-based session management
- Fine-grained permission control
- Integration with JSON-RPC and REST services

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   Client App    │────▶│ Identity Provider│────▶│ Session Service │
└─────────────────┘     └──────────────────┘     └─────────────────┘
                                                           │
                                                           ▼
                        ┌─────────────────┐       ┌─────────────────┐
                        │ JSON-RPC/REST   │◀──────│ JwtAuthProvider │
                        │    Service      │       └─────────────────┘
                        └─────────────────┘
```

## Quick Start

### 1. Add Dependencies

```toml
[dependencies]
# Core authentication traits
ras-auth-core = "0.1.0"
ras-identity-core = "0.1.1"

# Session management (required)
ras-identity-session = "0.2.0"

# Identity providers (choose what you need)
ras-identity-local = "0.2.0"
ras-identity-oauth2 = "0.1.2"

# For JSON-RPC services
ras-jsonrpc-core = "0.1.2"
```

### 2. Basic Setup with Local Authentication

```rust
use ras_identity_session::{JwtAuthProvider, SessionConfig, SessionService};
use ras_identity_local::LocalUserProvider;
use ras_auth_core::AuthProvider;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create session service with an example-length secret. Real services
    // should load a random secret from environment or secret storage.
    let session_config = SessionConfig::new("use-at-least-32-bytes-of-random-secret")?;
    let session_service = SessionService::new(session_config)?;
    
    // Create and configure local user provider
    let local_provider = LocalUserProvider::new();
    
    // Add some users
    local_provider.add_user(
        "admin".to_string(),
        "secure_password123".to_string(),
        Some("admin@example.com".to_string()),
        Some("Administrator".to_string())
    ).await?;
    
    // Register the provider with session service
    session_service.register_provider(Box::new(local_provider)).await;
    
    // Create JWT auth provider for your services
    let jwt_auth = JwtAuthProvider::new(Arc::new(session_service));
    
    // Now use jwt_auth with your JSON-RPC or REST services
    Ok(())
}
```

## Identity Providers

### Local User Provider

The local user provider handles username/password authentication with secure password hashing:

```rust
use ras_identity_core::IdentityProvider;
use ras_identity_local::LocalUserProvider;
use serde_json::json;

// Create provider
let provider = LocalUserProvider::new();

// Add users
provider.add_user(
    "alice".to_string(),
    "password123".to_string(),
    Some("alice@example.com".to_string()),
    Some("Alice Smith".to_string()),
).await?;

// Authenticate
let auth_payload = json!({
    "username": "alice",
    "password": "password123"
});

let identity = provider.verify(auth_payload).await?;
println!("Authenticated: {}", identity.display_name.unwrap_or_default());
```

**Security Features:**
- Argon2 password hashing
- Timing attack mitigation for missing users
- Uniform invalid-credentials errors
- Rate limiting (5 concurrent attempts)

### OAuth2 Provider

The OAuth2 provider supports external authentication providers like Google:

```rust
use ras_identity_core::{IdentityError, IdentityProvider};
use ras_identity_oauth2::{
    InMemoryStateStore, OAuth2Config, OAuth2Provider, OAuth2ProviderConfig, OAuth2Response,
};
use std::{collections::HashMap, sync::Arc};

// Configure OAuth2 provider
let google_config = OAuth2ProviderConfig {
    provider_id: "google".to_string(),
    client_id: std::env::var("GOOGLE_CLIENT_ID")
        .expect("GOOGLE_CLIENT_ID must be set"),
    client_secret: std::env::var("GOOGLE_CLIENT_SECRET")
        .expect("GOOGLE_CLIENT_SECRET must be set"),
    authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
    token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
    userinfo_endpoint: Some("https://www.googleapis.com/oauth2/v2/userinfo".to_string()),
    redirect_uri: "http://localhost:3000/auth/callback".to_string(),
    scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
    auth_params: HashMap::new(),
    use_pkce: true,
    user_info_mapping: None,
};

let config = OAuth2Config::new().add_provider(google_config);
let state_store = Arc::new(InMemoryStateStore::new());
let oauth_provider = OAuth2Provider::new(config, state_store);
```

**OAuth2 Flow:**

1. **Start Authorization:**
```rust
let start_payload = json!({
    "type": "StartFlow",
    "provider_id": "google"
});

match oauth_provider.verify(start_payload).await {
    Err(IdentityError::ProviderError(response_json)) => {
        let response: OAuth2Response = serde_json::from_str(&response_json)?;
        if let OAuth2Response::AuthorizationUrl { url, state } = response {
            // Redirect user to authorization URL and keep state for the callback.
            println!("Redirect to: {url}, state: {state}");
        }
    }
    Ok(_) => eprintln!("OAuth2 start flow completed without a redirect"),
    Err(err) => eprintln!("OAuth2 start flow failed: {err}"),
}
```

2. **Handle Callback:**
```rust
let callback_payload = json!({
    "type": "Callback",
    "provider_id": "google",
    "code": "authorization_code_from_provider",
    "state": "stored_csrf_state"
});

let identity = oauth_provider.verify(callback_payload).await?;
```

## Session Management

The `SessionService` orchestrates the login-to-session flow:

```rust
use chrono::Duration;
use ras_identity_session::{JwtAlgorithm, SessionConfig, SessionService};

// Configure session service
let config = SessionConfig {
    jwt_secret: "use-at-least-32-bytes-of-random-secret".to_string(),
    jwt_ttl: Duration::hours(1),
    refresh_enabled: false,
    enforce_active_sessions: true,
    algorithm: JwtAlgorithm::HS256,
};

let session_service = SessionService::new(config)?;

// Register multiple providers
session_service.register_provider(Box::new(local_provider)).await;
session_service.register_provider(Box::new(oauth_provider)).await;
```

### Creating Sessions

```rust
// Authenticate and create session
let auth_payload = json!({
    "username": "alice",
    "password": "password123"
});

let jwt_token = session_service.begin_session("local", auth_payload).await?;
println!("JWT Token: {}", jwt_token);

// Verify session
let claims = session_service.verify_session(&jwt_token).await?;
println!("Subject: {}", claims.sub);
println!("Permissions: {:?}", claims.permissions);

// End session (logout)
session_service.end_session(&claims.jti).await;
```

## Permission Management

Implement custom permission logic using the `UserPermissions` trait:

```rust
use async_trait::async_trait;
use ras_identity_core::{IdentityResult, UserPermissions, VerifiedIdentity};
use std::sync::Arc;

struct RoleBasedPermissions {
    // Your permission logic
}

#[async_trait]
impl UserPermissions for RoleBasedPermissions {
    async fn get_permissions(&self, identity: &VerifiedIdentity) -> IdentityResult<Vec<String>> {
        // Example: Grant permissions based on email domain
        match &identity.email {
            Some(email) if email.ends_with("@admin.com") => {
                Ok(vec!["admin".to_string(), "user".to_string()])
            }
            Some(_) => Ok(vec!["user".to_string()]),
            None => Ok(vec![]),
        }
    }
}

// Configure the session service before sharing it with handlers.
let mut session_service = SessionService::new(session_config)?;
session_service.set_permissions_provider(Arc::new(RoleBasedPermissions {}));
```

## Integration with Services

### JSON-RPC Service Integration

```rust
use ras_jsonrpc_macro::jsonrpc_service;
use ras_identity_session::JwtAuthProvider;

// Define your service with authentication
jsonrpc_service!({
    service_name: MyApiService,
    methods: [
        // Public method
        UNAUTHORIZED get_status(()) -> Status,
        
        // Requires authentication but no specific permission
        WITH_PERMISSIONS([]) get_profile(()) -> UserProfile,
        
        // Requires specific permissions
        WITH_PERMISSIONS(["admin"]) delete_user(DeleteUserRequest) -> (),
    ]
});

// Implement service
struct MyApiServiceImpl;

impl MyApiServiceTrait for MyApiServiceImpl {
    async fn get_status(&self) -> Result<Status, Error> {
        Ok(Status { healthy: true })
    }
    
    async fn get_profile(&self, user: &AuthenticatedUser, _request: ()) -> Result<UserProfile, Error> {
        // Access user.user_id, user.permissions, etc.
        Ok(UserProfile { 
            id: user.user_id.clone(),
            permissions: user.permissions.iter().cloned().collect(),
        })
    }
    
    async fn delete_user(&self, _user: &AuthenticatedUser, req: DeleteUserRequest) -> Result<(), Error> {
        // Only users with "admin" permission can reach here
        Ok(())
    }
}

// Set up with Axum
use axum::Router;

let jwt_auth = JwtAuthProvider::new(Arc::new(session_service));
let service = MyApiServiceImpl;

let app = Router::new()
    .nest("/api", 
        MyApiServiceBuilder::new(service)
            .base_url("/rpc")
            .auth_provider(jwt_auth)
            .build()?
    );
```

### REST Service Integration

```rust
use ras_rest_macro::rest_service;

rest_service!({
    service_name: UserApi,
    base_path: "/api/v1",
    endpoints: [
        // Public endpoint
        GET UNAUTHORIZED health() -> HealthResponse,
        
        // Authenticated endpoint with no specific permission
        GET WITH_PERMISSIONS([]) me() -> UserResponse,
        
        // Permission-protected endpoint
        DELETE WITH_PERMISSIONS(["admin"]) users/{id: String}() -> (),
    ]
});
```

## Service Composition Example

Here is a typical setup sketch showing how the identity pieces fit together with generated service routes. The request/response DTOs and handler bodies are application-specific.

```rust
use ras_identity_session::{JwtAlgorithm, JwtAuthProvider, SessionConfig, SessionService};
use ras_identity_local::LocalUserProvider;
use ras_jsonrpc_macro::jsonrpc_service;
use axum::{Router, routing::get};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

// Define your API
jsonrpc_service!({
    service_name: TodoService,
    methods: [
        UNAUTHORIZED health_check(()) -> HealthStatus,
        WITH_PERMISSIONS([]) list_todos(()) -> Vec<Todo>,
        WITH_PERMISSIONS(["user"]) create_todo(CreateTodoRequest) -> Todo,
        WITH_PERMISSIONS(["admin"]) delete_all_todos(()) -> (),
    ]
});

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Set up session service
    let session_config = SessionConfig {
        jwt_secret: std::env::var("JWT_SECRET")
            .expect("JWT_SECRET must be at least 32 bytes"),
        jwt_ttl: chrono::Duration::hours(1),
        refresh_enabled: false,
        enforce_active_sessions: true,
        algorithm: JwtAlgorithm::HS256,
    };

    // 2. Set up permissions
    use async_trait::async_trait;
    use ras_identity_core::{IdentityResult, UserPermissions, VerifiedIdentity};

    struct SimplePermissions;

    #[async_trait]
    impl UserPermissions for SimplePermissions {
        async fn get_permissions(&self, identity: &VerifiedIdentity) -> IdentityResult<Vec<String>> {
            match identity.subject.as_str() {
                "admin" => Ok(vec!["user".to_string(), "admin".to_string()]),
                _ => Ok(vec!["user".to_string()]),
            }
        }
    }

    let mut session_service = SessionService::new(session_config)?;
    session_service.set_permissions_provider(Arc::new(SimplePermissions));
    let session_service = Arc::new(session_service);

    // 3. Set up local authentication
    let local_provider = LocalUserProvider::new();
    local_provider
        .add_user(
            "user".to_string(),
            "password123".to_string(),
            Some("user@example.com".to_string()),
            Some("User".to_string()),
        )
        .await?;
    local_provider
        .add_user(
            "admin".to_string(),
            "admin12345".to_string(),
            Some("admin@example.com".to_string()),
            Some("Admin".to_string()),
        )
        .await?;

    session_service.register_provider(Box::new(local_provider)).await;
    
    // 4. Create authentication endpoints
    let auth_router = Router::new()
        .route("/login", get(login_handler))
        .route("/logout", get(logout_handler));
    
    // 5. Create API with authentication
    let jwt_auth = JwtAuthProvider::new(session_service.clone());
    let todo_service = TodoServiceImpl::new();
    
    let api_router = TodoServiceBuilder::new(todo_service)
        .base_url("/rpc")
        .auth_provider(jwt_auth)
        .build()?;
    
    // 6. Combine everything
    let app = Router::new()
        .nest("/auth", auth_router)
        .nest("/api", api_router)
        .layer(CorsLayer::permissive())
        .with_state(session_service);
    
    // 7. Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
```

## Best Practices

### 1. Security

- **Use strong JWT secrets**: Generate cryptographically secure secrets
- **Set appropriate TTLs**: Balance security and user experience
- **Enable HTTPS**: Always use TLS in production
- **Validate permissions**: Check permissions at the service level
- **Handle errors gracefully**: Don't leak information in error messages

### 2. Configuration

```rust
// Use environment variables for sensitive config
let mut config = SessionConfig::new(
    std::env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
)?;
config.jwt_ttl = chrono::Duration::seconds(
    std::env::var("JWT_TTL_SECONDS")
        .unwrap_or_else(|_| "3600".to_string())
        .parse()?,
);
config.refresh_enabled = false;
```

### 3. Error Handling

```rust
use ras_identity_core::IdentityError;
use ras_identity_session::SessionError;

match session_service.begin_session("local", payload).await {
    Ok(token) => {
        // Success
    }
    Err(SessionError::IdentityError(IdentityError::InvalidCredentials)) => {
        // Wrong username/password
    }
    Err(SessionError::IdentityError(IdentityError::ProviderNotFound(_))) => {
        // Provider not registered
    }
    Err(e) => {
        // Other errors
    }
}
```

### 4. Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_authentication_flow() {
        // Set up test providers
        let provider = LocalUserProvider::new();
        provider
            .add_user("test".to_string(), "test123".to_string(), None, None)
            .await
            .unwrap();

        let session_config =
            SessionConfig::new("test-secret-key-that-is-at-least-32-bytes").unwrap();
        let session_service = SessionService::new(session_config).unwrap();
        session_service.register_provider(Box::new(provider)).await;
        
        // Test authentication
        let token = session_service.begin_session("local", json!({
            "username": "test",
            "password": "test123"
        })).await.unwrap();
        
        // Verify token
        let claims = session_service.verify_session(&token).await.unwrap();
        assert_eq!(claims.sub, "test");
    }
}
```

## Troubleshooting

### Common Issues

1. **"Provider not found" error**
   - Ensure you've registered the provider with `session_service.register_provider()`
   - Check the provider ID matches (e.g., "local" for LocalUserProvider)

2. **JWT validation failures**
   - Verify the JWT secret is consistent across services
   - Check token hasn't expired; `SessionConfig::new` defaults to 24 hours, while the examples above configure one hour explicitly
   - Ensure the token is passed in the correct format

3. **Permission denied errors**
   - Verify your `UserPermissions` implementation returns expected permissions
   - Check the method annotation matches required permissions
   - Use `WITH_PERMISSIONS([])` for methods that only need login, not specific permissions

4. **OAuth2 redirect issues**
   - Ensure redirect URLs are correctly configured in provider settings
   - Check CORS settings allow the callback domain
   - Verify state parameter is preserved through the flow

## Advanced Topics

### Custom Identity Providers

Implement the `IdentityProvider` trait for custom authentication:

```rust
use ras_identity_core::{IdentityError, IdentityProvider, IdentityResult, VerifiedIdentity};
use async_trait::async_trait;

struct LdapProvider {
    // LDAP configuration
}

#[async_trait]
impl IdentityProvider for LdapProvider {
    fn provider_id(&self) -> &str {
        "ldap"
    }

    async fn verify(&self, payload: serde_json::Value) -> IdentityResult<VerifiedIdentity> {
        let username = payload
            .get("username")
            .and_then(|value| value.as_str())
            .ok_or(IdentityError::InvalidPayload)?;

        Ok(VerifiedIdentity {
            provider_id: self.provider_id().to_string(),
            subject: username.to_string(),
            email: None,
            display_name: Some(username.to_string()),
            metadata: None,
        })
    }
}
```

### Session Revocation

Implement immediate session revocation:

```rust
// End specific session
let claims = session_service.verify_session(&jwt_token).await?;
session_service.end_session(&claims.jti).await;

// End all sessions for a user
// (Requires custom implementation tracking user->session mapping)
```

### Refresh Tokens

`SessionConfig::refresh_enabled` is reserved for applications that add their own refresh-token storage and rotation. The current `SessionService` issues and verifies access JWTs; long-lived refresh tokens should be implemented as an application-level flow with server-side persistence and token rotation.

```rust
let mut config = SessionConfig::new("use-at-least-32-bytes-of-random-secret")?;
config.refresh_enabled = false;
```

## Conclusion

The RAS identity crates provide local authentication, OAuth2 callbacks, JWT
sessions, and permission lookup traits that can be composed for application
authentication flows. Start with basic local authentication, then add OAuth2
providers and custom permission logic as needed.

For more examples, check out:
- [`examples/oauth2-demo`](../examples/oauth2-demo/) - OAuth2 integration demo
- [`examples/basic-jsonrpc`](../examples/basic-jsonrpc/) - JSON-RPC with authentication
- [`examples/bidirectional-chat`](../examples/bidirectional-chat/) - WebSocket authentication
