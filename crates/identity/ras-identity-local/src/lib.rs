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

/// Redacting `Debug` so the Argon2 `password_hash` never lands in logs.
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
mod tests;
