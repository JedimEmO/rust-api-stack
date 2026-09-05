//! Local user identity provider with username/password authentication.

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use async_trait::async_trait;
use rand_core::OsRng;
use ras_identity_core::{IdentityError, IdentityProvider, IdentityResult, VerifiedIdentity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum accepted password length in bytes (I4). Longer inputs are rejected before
/// hashing so a client cannot make the server burn Argon2 time on multi-megabyte inputs.
pub const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct LocalUser {
    pub username: String,
    /// Argon2 PHC string. Never serialized (I1a); still required on deserialize.
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Redacting `Debug` so the Argon2 `password_hash` never lands in logs (L1).
impl fmt::Debug for LocalUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalUser")
            .field("username", &self.username)
            .field("password_hash", &"[REDACTED]")
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[derive(Deserialize)]
pub struct LocalAuthPayload {
    pub username: String,
    pub password: String,
}

/// Redacting `Debug` so the plaintext password never lands in logs (I2).
impl fmt::Debug for LocalAuthPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalAuthPayload")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Errors returned when managing local users.
#[derive(Debug)]
pub enum LocalUserError {
    /// A user with the requested username already exists.
    UserAlreadyExists { username: String },
    /// Password hashing failed while creating the user.
    PasswordHash(argon2::password_hash::Error),
    /// The password exceeds [`MAX_PASSWORD_BYTES`].
    PasswordTooLong { max_bytes: usize },
    /// The blocking hashing task was cancelled or panicked.
    HashTaskFailed,
}

impl fmt::Display for LocalUserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserAlreadyExists { username } => {
                write!(f, "user '{username}' already exists")
            }
            Self::PasswordHash(error) => write!(f, "failed to hash password: {error}"),
            Self::PasswordTooLong { max_bytes } => {
                write!(f, "password exceeds maximum length of {max_bytes} bytes")
            }
            Self::HashTaskFailed => write!(f, "password hashing task failed"),
        }
    }
}

impl Error for LocalUserError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PasswordHash(error) => Some(error),
            Self::UserAlreadyExists { .. }
            | Self::PasswordTooLong { .. }
            | Self::HashTaskFailed => None,
        }
    }
}

impl From<argon2::password_hash::Error> for LocalUserError {
    fn from(error: argon2::password_hash::Error) -> Self {
        Self::PasswordHash(error)
    }
}

#[derive(Clone)]
pub struct LocalUserProvider {
    users: Arc<RwLock<HashMap<String, LocalUser>>>,
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl LocalUserProvider {
    pub fn new() -> Self {
        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            semaphore: Arc::new(tokio::sync::Semaphore::new(5)),
        }
    }

    pub async fn add_user(
        &self,
        username: String,
        password: String,
        email: Option<String>,
        display_name: Option<String>,
    ) -> Result<(), LocalUserError> {
        if password.len() > MAX_PASSWORD_BYTES {
            return Err(LocalUserError::PasswordTooLong {
                max_bytes: MAX_PASSWORD_BYTES,
            });
        }

        {
            let users = self.users.read().await;
            if users.contains_key(&username) {
                return Err(LocalUserError::UserAlreadyExists { username });
            }
        }

        // Argon2 is CPU-bound; keep it off the async executor (I4).
        let password_hash = tokio::task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);
            Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
        })
        .await
        .map_err(|_| LocalUserError::HashTaskFailed)??;

        let user = LocalUser {
            username: username.clone(),
            password_hash,
            email,
            display_name,
            metadata: None,
        };

        let mut users = self.users.write().await;
        if users.contains_key(&username) {
            return Err(LocalUserError::UserAlreadyExists { username });
        }

        users.insert(username, user);

        Ok(())
    }

    pub async fn remove_user(&self, username: &str) -> Option<LocalUser> {
        let mut users = self.users.write().await;
        users.remove(username)
    }

    async fn verify_user(&self, username: &str, password: &str) -> IdentityResult<LocalUser> {
        let _semlock =
            self.semaphore.clone().acquire_owned().await.map_err(|_| {
                IdentityError::ProviderError("local auth limiter closed".to_string())
            })?;

        // Reject oversized passwords before spending Argon2 time on them (I4). Same error
        // as a wrong password so nothing is leaked about the account.
        if password.len() > MAX_PASSWORD_BYTES {
            return Err(IdentityError::InvalidCredentials);
        }

        // Verify missing users against a fixed sentinel hash to keep timing consistent.
        const SENTINEL_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$9QsJRKgzJkKaOUvlp7gl2Q$qmE3qIFBNJ6nZYbLYXEI2uo0zZc7T0Q8LU1ZsqsZ3QE";

        // Clone the stored hash out of the lock so verification never holds it (I4).
        let (user, password_hash) = {
            let users = self.users.read().await;
            match users.get(username) {
                Some(user) => (Some(user.clone()), user.password_hash.clone()),
                None => (None, SENTINEL_HASH.to_string()),
            }
        };

        // Argon2 is CPU-bound; run it on the blocking pool (I4).
        let password = password.to_string();
        let password_valid = tokio::task::spawn_blocking(move || {
            let parsed_hash = PasswordHash::new(&password_hash)
                .map_err(|e| IdentityError::ProviderError(e.to_string()))?;
            Ok::<bool, IdentityError>(
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed_hash)
                    .is_ok(),
            )
        })
        .await
        .map_err(|_| {
            IdentityError::ProviderError("password verification task failed".to_string())
        })??;

        // Only succeed if both user exists AND password is valid.
        if password_valid {
            user.ok_or(IdentityError::InvalidCredentials)
        } else {
            // Always return the same error regardless of whether user exists or password is wrong
            Err(IdentityError::InvalidCredentials)
        }
    }
}

impl Default for LocalUserProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IdentityProvider for LocalUserProvider {
    fn provider_id(&self) -> &str {
        "local"
    }

    async fn verify(&self, auth_payload: serde_json::Value) -> IdentityResult<VerifiedIdentity> {
        let payload: LocalAuthPayload =
            serde_json::from_value(auth_payload).map_err(|_| IdentityError::InvalidPayload)?;

        let user = self
            .verify_user(&payload.username, &payload.password)
            .await?;

        Ok(VerifiedIdentity {
            provider_id: self.provider_id().to_string(),
            subject: user.username,
            email: user.email,
            display_name: user.display_name,
            metadata: user.metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_password_hash() {
        let user = LocalUser {
            username: "alice".to_string(),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$secretsecret$hashhashhash".to_string(),
            email: None,
            display_name: None,
            metadata: None,
        };
        let debug = format!("{user:?}");
        assert!(!debug.contains("hashhashhash"));
        assert!(!debug.contains("$argon2id$"));
        assert!(debug.contains("[REDACTED]"));
        assert!(debug.contains("alice"));
    }

    #[test]
    fn i1a_local_user_serialize_omits_password_hash() {
        let user = LocalUser {
            username: "alice".to_string(),
            password_hash: "$argon2id$v=19$m=19456,t=2,p=1$secretsecret$hashhashhash".to_string(),
            email: Some("alice@example.com".to_string()),
            display_name: None,
            metadata: None,
        };
        let json = serde_json::to_value(&user).unwrap();
        assert!(json.get("password_hash").is_none());
        assert!(!json.to_string().contains("hashhashhash"));
        assert_eq!(json["username"], "alice");

        // Deserialize still requires the hash.
        let full = serde_json::json!({
            "username": "alice",
            "password_hash": "$argon2id$x",
            "email": null,
            "display_name": null,
            "metadata": null
        });
        let parsed: LocalUser = serde_json::from_value(full).unwrap();
        assert_eq!(parsed.password_hash, "$argon2id$x");
        assert!(serde_json::from_value::<LocalUser>(json).is_err());
    }

    #[test]
    fn i2_login_payload_debug_redacts_password() {
        let payload = LocalAuthPayload {
            username: "alice".to_string(),
            password: "hunter2-super-secret".to_string(),
        };
        let debug = format!("{payload:?}");
        assert!(!debug.contains("hunter2"));
        assert!(debug.contains("[REDACTED]"));
        assert!(debug.contains("alice"));
    }

    #[tokio::test]
    async fn i4_oversized_password_rejected_before_hashing() {
        let provider = setup_test_provider().await;

        let too_long = "x".repeat(MAX_PASSWORD_BYTES + 1);
        let result = provider
            .add_user("bob".to_string(), too_long.clone(), None, None)
            .await;
        assert!(matches!(
            result,
            Err(LocalUserError::PasswordTooLong { max_bytes }) if max_bytes == MAX_PASSWORD_BYTES
        ));

        let result = provider
            .verify(serde_json::json!({ "username": "testuser", "password": too_long }))
            .await;
        assert!(matches!(result, Err(IdentityError::InvalidCredentials)));

        // Exactly at the limit is still accepted.
        let at_limit = "y".repeat(MAX_PASSWORD_BYTES);
        provider
            .add_user("carol".to_string(), at_limit.clone(), None, None)
            .await
            .unwrap();
        assert!(
            provider
                .verify(serde_json::json!({ "username": "carol", "password": at_limit }))
                .await
                .is_ok()
        );
    }

    async fn setup_test_provider() -> LocalUserProvider {
        let provider = LocalUserProvider::new();

        // Add test users
        provider
            .add_user(
                "testuser".to_string(),
                "password123".to_string(),
                Some("test@example.com".to_string()),
                Some("Test User".to_string()),
            )
            .await
            .unwrap();

        provider
            .add_user(
                "alice".to_string(),
                "supersecret".to_string(),
                Some("alice@example.com".to_string()),
                Some("Alice Smith".to_string()),
            )
            .await
            .unwrap();

        provider
    }

    #[tokio::test]
    async fn test_basic_authentication_success() {
        let provider = setup_test_provider().await;

        let auth_payload = serde_json::json!({
            "username": "testuser",
            "password": "password123"
        });

        let identity = provider.verify(auth_payload).await.unwrap();

        assert_eq!(identity.subject, "testuser");
        assert_eq!(identity.email.as_deref(), Some("test@example.com"));
        assert_eq!(identity.display_name.as_deref(), Some("Test User"));
        assert_eq!(identity.provider_id, "local");
    }

    #[tokio::test]
    async fn test_duplicate_user_is_rejected() {
        let provider = setup_test_provider().await;

        let result = provider
            .add_user(
                "testuser".to_string(),
                "replacement-password".to_string(),
                Some("other@example.com".to_string()),
                Some("Other User".to_string()),
            )
            .await;

        assert!(matches!(
            result,
            Err(LocalUserError::UserAlreadyExists { username }) if username == "testuser"
        ));

        let original_password_payload = serde_json::json!({
            "username": "testuser",
            "password": "password123"
        });
        assert!(provider.verify(original_password_payload).await.is_ok());

        let replacement_password_payload = serde_json::json!({
            "username": "testuser",
            "password": "replacement-password"
        });
        assert!(matches!(
            provider.verify(replacement_password_payload).await,
            Err(IdentityError::InvalidCredentials)
        ));
    }

    #[tokio::test]
    async fn remove_user_deletes_credentials_and_returns_user() {
        let provider = setup_test_provider().await;

        let removed = provider.remove_user("alice").await.expect("user removed");
        assert_eq!(removed.username, "alice");
        assert_eq!(removed.email.as_deref(), Some("alice@example.com"));

        let payload = serde_json::json!({
            "username": "alice",
            "password": "supersecret"
        });
        let result = provider.verify(payload).await;
        assert!(matches!(result, Err(IdentityError::InvalidCredentials)));
        assert!(provider.remove_user("alice").await.is_none());
    }

    #[tokio::test]
    async fn default_provider_starts_empty_with_local_provider_id() {
        let provider = LocalUserProvider::default();
        assert_eq!(provider.provider_id(), "local");

        let result = provider
            .verify(serde_json::json!({
                "username": "missing",
                "password": "irrelevant"
            }))
            .await;
        assert!(matches!(result, Err(IdentityError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn malformed_stored_password_hash_returns_provider_error() {
        let provider = LocalUserProvider::new();
        provider.users.write().await.insert(
            "broken".to_string(),
            LocalUser {
                username: "broken".to_string(),
                password_hash: "not-a-phc-password-hash".to_string(),
                email: None,
                display_name: None,
                metadata: None,
            },
        );

        let result = provider
            .verify(serde_json::json!({
                "username": "broken",
                "password": "password123"
            }))
            .await;

        assert!(matches!(
            result,
            Err(IdentityError::ProviderError(message))
                if message.contains("password hash") || message.contains("PHC")
        ));
    }

    #[tokio::test]
    async fn closed_limiter_returns_provider_error() {
        let provider = setup_test_provider().await;
        provider.semaphore.close();

        let result = provider
            .verify(serde_json::json!({
                "username": "testuser",
                "password": "password123"
            }))
            .await;

        assert!(matches!(
            result,
            Err(IdentityError::ProviderError(message))
                if message == "local auth limiter closed"
        ));
    }

    #[test]
    fn local_user_error_display_and_source_are_stable() {
        use std::error::Error as _;

        let duplicate = LocalUserError::UserAlreadyExists {
            username: "alice".to_string(),
        };
        assert_eq!(duplicate.to_string(), "user 'alice' already exists");
        assert!(duplicate.source().is_none());

        let parse_error = PasswordHash::new("not-a-phc-password-hash").unwrap_err();
        let hash_error = LocalUserError::from(parse_error);
        assert!(hash_error.to_string().contains("failed to hash password"));
        assert!(hash_error.source().is_some());
    }

    #[tokio::test]
    async fn test_wrong_password_fails() {
        let provider = setup_test_provider().await;

        let bad_payload = serde_json::json!({
            "username": "testuser",
            "password": "wrongpassword"
        });

        let result = provider.verify(bad_payload).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            IdentityError::InvalidCredentials => {} // Expected
            other => panic!("Expected InvalidCredentials, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_username_enumeration_prevention() {
        let provider = setup_test_provider().await;

        // Test with non-existent username
        let nonexistent_user_payload = serde_json::json!({
            "username": "nonexistentuser",
            "password": "anypassword"
        });

        // Test with existing username but wrong password
        let wrong_password_payload = serde_json::json!({
            "username": "testuser",
            "password": "wrongpassword"
        });

        let result1 = provider.verify(nonexistent_user_payload).await;
        let result2 = provider.verify(wrong_password_payload).await;

        // Both should fail with the same error type
        assert!(result1.is_err());
        assert!(result2.is_err());

        let err1 = result1.unwrap_err();
        let err2 = result2.unwrap_err();

        // Both should be InvalidCredentials errors
        assert!(matches!(err1, IdentityError::InvalidCredentials));
        assert!(matches!(err2, IdentityError::InvalidCredentials));

        // Error messages should be identical
        assert_eq!(err1.to_string(), err2.to_string());
    }

    #[cfg(feature = "timing-tests")]
    #[tokio::test]
    #[ignore = "timing-sensitive statistical check; run explicitly on a quiet machine"]
    async fn test_timing_attack_resistance() {
        use std::time::{Duration, Instant};

        let provider = setup_test_provider().await;

        const NUM_ATTEMPTS: usize = 10;
        let mut nonexistent_times = Vec::new();
        let mut wrong_password_times = Vec::new();

        // Measure timing for non-existent users
        for i in 0..NUM_ATTEMPTS {
            let payload = serde_json::json!({
                "username": format!("nonexistentuser{}", i),
                "password": "anypassword"
            });

            let start = Instant::now();
            let _ = provider.verify(payload).await;
            let duration = start.elapsed();
            nonexistent_times.push(duration);
        }

        // Measure timing for wrong passwords with existing users
        for i in 0..NUM_ATTEMPTS {
            let payload = serde_json::json!({
                "username": "testuser",
                "password": format!("wrongpassword{}", i)
            });

            let start = Instant::now();
            let _ = provider.verify(payload).await;
            let duration = start.elapsed();
            wrong_password_times.push(duration);
        }

        // Calculate average times
        let avg_nonexistent = nonexistent_times.iter().sum::<Duration>() / NUM_ATTEMPTS as u32;
        let avg_wrong_password =
            wrong_password_times.iter().sum::<Duration>() / NUM_ATTEMPTS as u32;

        // The difference should be small (less than 10ms typically for Argon2)
        let time_diff = avg_nonexistent.abs_diff(avg_wrong_password);

        println!("Average time for nonexistent user: {:?}", avg_nonexistent);
        println!("Average time for wrong password: {:?}", avg_wrong_password);
        println!("Time difference: {:?}", time_diff);

        // Assert that timing difference is reasonable (less than 50ms)
        // This is generous but accounts for system variance
        assert!(
            time_diff < Duration::from_millis(50),
            "Timing difference too large: {:?}. This could enable timing attacks.",
            time_diff
        );
    }

    #[cfg(feature = "timing-tests")]
    #[tokio::test]
    async fn test_brute_force_simulation() {
        let provider = setup_test_provider().await;

        const ATTACK_ATTEMPTS: usize = 50;
        let mut consecutive_failures = 0;
        let mut error_consistency = true;

        // Simulate brute force attack on known username
        for i in 0..ATTACK_ATTEMPTS {
            let payload = serde_json::json!({
                "username": "testuser",
                "password": format!("bruteforce_attempt_{}", i)
            });

            let result = provider.verify(payload).await;

            if let Err(error) = result {
                consecutive_failures += 1;

                // Ensure all failures are consistent
                if !matches!(error, IdentityError::InvalidCredentials) {
                    error_consistency = false;
                }
            } else {
                // Should not succeed with random passwords
                panic!("Brute force attempt unexpectedly succeeded");
            }
        }

        assert_eq!(consecutive_failures, ATTACK_ATTEMPTS);
        assert!(
            error_consistency,
            "Error types were not consistent across brute force attempts"
        );
    }

    #[tokio::test]
    async fn test_malformed_payload_handling() {
        let provider = setup_test_provider().await;

        // Test with missing username
        let missing_username = serde_json::json!({
            "password": "password123"
        });

        let result = provider.verify(missing_username).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IdentityError::InvalidPayload));

        // Test with missing password
        let missing_password = serde_json::json!({
            "username": "testuser"
        });

        let result = provider.verify(missing_password).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IdentityError::InvalidPayload));

        // Test with wrong field names
        let wrong_fields = serde_json::json!({
            "user": "testuser",
            "pass": "password123"
        });

        let result = provider.verify(wrong_fields).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IdentityError::InvalidPayload));

        // Test with completely invalid JSON structure
        let invalid_structure = serde_json::json!("just a string");

        let result = provider.verify(invalid_structure).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IdentityError::InvalidPayload));
    }

    #[tokio::test]
    async fn test_empty_credentials() {
        let provider = setup_test_provider().await;

        // Test with empty username
        let empty_username = serde_json::json!({
            "username": "",
            "password": "password123"
        });

        let result = provider.verify(empty_username).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IdentityError::InvalidCredentials
        ));

        // Test with empty password
        let empty_password = serde_json::json!({
            "username": "testuser",
            "password": ""
        });

        let result = provider.verify(empty_password).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IdentityError::InvalidCredentials
        ));

        // Test with both empty
        let both_empty = serde_json::json!({
            "username": "",
            "password": ""
        });

        let result = provider.verify(both_empty).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IdentityError::InvalidCredentials
        ));
    }

    #[tokio::test]
    async fn test_special_characters_in_credentials() {
        let provider = LocalUserProvider::new();

        // Add user with special characters in username and password
        provider
            .add_user(
                "user@domain.com".to_string(),
                "p@ssw0rd!#$%".to_string(),
                None,
                None,
            )
            .await
            .unwrap();

        // Test successful authentication with special characters
        let payload = serde_json::json!({
            "username": "user@domain.com",
            "password": "p@ssw0rd!#$%"
        });

        let result = provider.verify(payload).await;
        assert!(result.is_ok());

        // Test with SQL injection-like patterns (should be safely handled)
        let sql_injection_attempt = serde_json::json!({
            "username": "user@domain.com'; DROP TABLE users; --",
            "password": "p@ssw0rd!#$%"
        });

        let result = provider.verify(sql_injection_attempt).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IdentityError::InvalidCredentials
        ));
    }

    #[tokio::test]
    async fn test_very_long_credentials() {
        let provider = setup_test_provider().await;

        // Test with extremely long username
        let long_username = "a".repeat(10000);
        let long_username_payload = serde_json::json!({
            "username": long_username,
            "password": "password123"
        });

        let result = provider.verify(long_username_payload).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IdentityError::InvalidCredentials
        ));

        // Test with extremely long password
        let long_password = "b".repeat(10000);
        let long_password_payload = serde_json::json!({
            "username": "testuser",
            "password": long_password
        });

        let result = provider.verify(long_password_payload).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IdentityError::InvalidCredentials
        ));
    }

    #[tokio::test]
    async fn test_concurrent_authentication_attempts() {
        let provider = setup_test_provider().await;
        let provider = Arc::new(provider);

        const CONCURRENT_ATTEMPTS: usize = 20;
        let mut handles = Vec::new();

        // Launch concurrent authentication attempts
        for i in 0..CONCURRENT_ATTEMPTS {
            let provider_clone = Arc::clone(&provider);
            let handle = tokio::spawn(async move {
                let payload = if i % 2 == 0 {
                    // Half valid, half invalid
                    serde_json::json!({
                        "username": "testuser",
                        "password": "password123"
                    })
                } else {
                    serde_json::json!({
                        "username": "testuser",
                        "password": format!("wrong_password_{}", i)
                    })
                };

                provider_clone.verify(payload).await
            });
            handles.push(handle);
        }

        // Collect results
        let mut successful_auths = 0;
        let mut failed_auths = 0;

        for handle in handles {
            let result = handle.await.unwrap();
            match result {
                Ok(_) => successful_auths += 1,
                Err(IdentityError::InvalidCredentials) => failed_auths += 1,
                Err(other) => panic!("Unexpected error: {:?}", other),
            }
        }

        // Half of the attempts use valid credentials and half use invalid credentials.
        assert_eq!(successful_auths, CONCURRENT_ATTEMPTS / 2);
        assert_eq!(failed_auths, CONCURRENT_ATTEMPTS / 2);
    }
}
