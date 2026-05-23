//! Configuration module for the bidirectional chat server
//!
//! This module provides the configuration loading paths used by the example:
//! - Environment variables (with CHAT_ prefix)
//! - Configuration file (config.toml)
//! - Default values
//! - Validation
//!
//! Environment variables take precedence over config file values.

use anyhow::{Context, Result};
use config::{Config as ConfigBuilder, Environment, File};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Main configuration struct for the chat server
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Server configuration
    pub server: ServerConfig,

    /// Authentication configuration
    pub auth: AuthConfig,

    /// Chat-specific configuration
    pub chat: ChatConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// Admin configuration
    pub admin: AdminConfig,

    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,
}

/// Server network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Host to bind to (default: 127.0.0.1)
    #[serde(default = "default_host")]
    pub host: IpAddr,

    /// Port to bind to (default: 3000)
    #[serde(default = "default_port")]
    pub port: u16,

    /// CORS configuration
    #[serde(default)]
    pub cors: CorsConfig,
}

/// CORS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// Whether to allow any origin (default: true)
    #[serde(default = "default_true")]
    pub allow_any_origin: bool,

    /// Specific allowed origins (only used if allow_any_origin is false)
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// JWT secret key
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,

    /// JWT TTL in seconds (default: 86400 = 24 hours)
    #[serde(default = "default_jwt_ttl")]
    pub jwt_ttl_seconds: i64,

    /// Whether to enable refresh tokens
    #[serde(default = "default_true")]
    pub refresh_enabled: bool,

    /// JWT algorithm (default: HS256)
    #[serde(default = "default_jwt_algorithm")]
    pub jwt_algorithm: String,
}

/// Chat-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatConfig {
    /// Data directory for persistence
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// Maximum message length in characters
    #[serde(default = "default_max_message_length")]
    pub max_message_length: usize,

    /// Maximum room name length in characters
    #[serde(default = "default_max_room_name_length")]
    pub max_room_name_length: usize,

    /// Maximum users per room (0 = unlimited)
    #[serde(default)]
    pub max_users_per_room: usize,

    /// Default room names to create on startup
    #[serde(default = "default_rooms")]
    pub default_rooms: Vec<RoomConfig>,

    /// Whether to persist messages
    #[serde(default = "default_true")]
    pub persist_messages: bool,

    /// Whether to persist room state
    #[serde(default = "default_true")]
    pub persist_rooms: bool,

    /// Whether to persist user profiles
    #[serde(default = "default_true")]
    pub persist_profiles: bool,
}

/// Room configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomConfig {
    /// Room ID
    pub id: String,

    /// Room display name
    pub name: String,

    /// Room description (optional)
    #[serde(default)]
    pub description: Option<String>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (pretty, json, compact)
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Whether to include timestamps
    #[serde(default = "default_true")]
    pub timestamps: bool,

    /// Whether to include target module
    #[serde(default = "default_true")]
    pub target: bool,

    /// Whether to include line numbers
    #[serde(default = "default_true")]
    pub line_numbers: bool,

    /// Whether to include thread IDs
    #[serde(default = "default_true")]
    pub thread_ids: bool,
}

/// Admin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdminConfig {
    /// Initial admin users to create
    #[serde(default)]
    pub users: Vec<AdminUser>,

    /// Whether to auto-create admin users on startup
    #[serde(default = "default_true")]
    pub auto_create: bool,
}

/// Admin user configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUser {
    /// Username
    pub username: String,

    /// Password (will be hashed)
    pub password: String,

    /// Email (optional)
    #[serde(default)]
    pub email: Option<String>,

    /// Display name (optional)
    #[serde(default)]
    pub display_name: Option<String>,

    /// Permissions to grant
    #[serde(default = "default_admin_permissions")]
    pub permissions: Vec<String>,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Messages per minute per authenticated chat user.
    #[serde(default = "default_messages_per_minute")]
    pub messages_per_minute: u32,

    /// Reserved for deployment-level connection limiting; validated but not enforced by the chat service.
    #[serde(default = "default_connections_per_ip")]
    pub connections_per_ip: u32,

    /// Reserved for deployment-level login throttling; validated but not enforced by the chat service.
    #[serde(default = "default_login_attempts_per_hour")]
    pub login_attempts_per_hour: u32,
}

// Default value functions
fn default_host() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

fn default_port() -> u16 {
    3000
}

fn default_true() -> bool {
    true
}

fn default_jwt_secret() -> String {
    warn!("Using default JWT secret - this is insecure for production!");
    "dev-secret-key-change-in-production".to_string()
}

fn default_jwt_ttl() -> i64 {
    86400 // 24 hours
}

fn default_jwt_algorithm() -> String {
    "HS256".to_string()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("./chat_data")
}

fn default_max_message_length() -> usize {
    1000
}

fn default_max_room_name_length() -> usize {
    50
}

fn default_rooms() -> Vec<RoomConfig> {
    vec![RoomConfig {
        id: "general".to_string(),
        name: "General".to_string(),
        description: Some("General discussion room".to_string()),
    }]
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_admin_permissions() -> Vec<String> {
    vec![
        "admin".to_string(),
        "moderator".to_string(),
        "user".to_string(),
    ]
}

fn default_messages_per_minute() -> u32 {
    30
}

fn default_connections_per_ip() -> u32 {
    10
}

fn default_login_attempts_per_hour() -> u32 {
    20
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            cors: CorsConfig::default(),
        }
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allow_any_origin: true,
            allowed_origins: vec![],
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: default_jwt_secret(),
            jwt_ttl_seconds: default_jwt_ttl(),
            refresh_enabled: true,
            jwt_algorithm: default_jwt_algorithm(),
        }
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            max_message_length: default_max_message_length(),
            max_room_name_length: default_max_room_name_length(),
            max_users_per_room: 0,
            default_rooms: default_rooms(),
            persist_messages: true,
            persist_rooms: true,
            persist_profiles: true,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            timestamps: true,
            target: true,
            line_numbers: true,
            thread_ids: true,
        }
    }
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            users: vec![],
            auto_create: true,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            messages_per_minute: default_messages_per_minute(),
            connections_per_ip: default_connections_per_ip(),
            login_attempts_per_hour: default_login_attempts_per_hour(),
        }
    }
}

impl Config {
    /// Load configuration from environment and optional config file
    pub fn load() -> Result<Self> {
        let mut builder = ConfigBuilder::builder();

        // Check for config file
        let config_path =
            std::env::var("CHAT_CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());

        if std::path::Path::new(&config_path).exists() {
            info!("Loading configuration from {}", config_path);
            builder = builder.add_source(File::with_name(&config_path));
        } else {
            debug!("No config file found at {}, using defaults", config_path);
        }

        // Add environment variables with CHAT_ prefix
        builder = builder.add_source(
            Environment::with_prefix("CHAT")
                .separator("__") // Use __ for nested values, e.g., CHAT__SERVER__PORT
                .try_parsing(true), // Parse strings to proper types
        );

        // Build and deserialize
        let config = builder.build().context("Failed to build configuration")?;

        let mut settings: Config = config
            .try_deserialize()
            .context("Failed to deserialize configuration")?;

        // Apply any direct environment variable overrides that don't fit the pattern
        settings.apply_env_overrides()?;

        // Validate configuration
        settings.validate()?;

        Ok(settings)
    }

    /// Apply direct environment variable overrides
    fn apply_env_overrides(&mut self) -> Result<()> {
        // Handle legacy environment variables for backward compatibility
        if let Ok(host) = std::env::var("HOST") {
            info!("Using HOST environment variable");
            self.server.host = host.parse().context("Invalid HOST value")?;
        }

        if let Ok(port) = std::env::var("PORT") {
            info!("Using PORT environment variable");
            self.server.port = port.parse().context("Invalid PORT value")?;
        }

        if let Ok(jwt_secret) = std::env::var("JWT_SECRET") {
            info!("Using JWT_SECRET environment variable");
            self.auth.jwt_secret = jwt_secret;
        }

        if let Ok(data_dir) = std::env::var("CHAT_DATA_DIR") {
            info!("Using CHAT_DATA_DIR environment variable");
            self.chat.data_dir = PathBuf::from(data_dir);
        }

        if let Ok(log_level) = std::env::var("RUST_LOG") {
            info!("Using RUST_LOG environment variable");
            self.logging.level = log_level;
        }

        Ok(())
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        // Validate port
        if self.server.port == 0 {
            anyhow::bail!("Server port cannot be 0");
        }

        // Validate JWT secret in production
        if !cfg!(debug_assertions) && self.auth.jwt_secret == default_jwt_secret() {
            anyhow::bail!("JWT secret must be changed from default in production");
        }

        // Validate JWT TTL
        if self.auth.jwt_ttl_seconds <= 0 {
            anyhow::bail!("JWT TTL must be positive");
        }

        // Validate message length
        if self.chat.max_message_length == 0 {
            anyhow::bail!("Maximum message length must be greater than 0");
        }

        // Validate room name length
        if self.chat.max_room_name_length == 0 {
            anyhow::bail!("Maximum room name length must be greater than 0");
        }

        // Validate log level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        let level_lower = self.logging.level.to_lowercase();
        if !valid_levels.contains(&level_lower.as_str()) {
            anyhow::bail!(
                "Invalid log level '{}'. Must be one of: {:?}",
                self.logging.level,
                valid_levels
            );
        }

        // Validate log format
        let valid_formats = ["pretty", "json", "compact"];
        let format_lower = self.logging.format.to_lowercase();
        if !valid_formats.contains(&format_lower.as_str()) {
            anyhow::bail!(
                "Invalid log format '{}'. Must be one of: {:?}",
                self.logging.format,
                valid_formats
            );
        }

        // Validate rate limiting if enabled
        if self.rate_limit.enabled {
            if self.rate_limit.messages_per_minute == 0 {
                anyhow::bail!(
                    "Messages per minute must be greater than 0 when rate limiting is enabled"
                );
            }
            if self.rate_limit.connections_per_ip == 0 {
                anyhow::bail!(
                    "Connections per IP must be greater than 0 when rate limiting is enabled"
                );
            }
            if self.rate_limit.login_attempts_per_hour == 0 {
                anyhow::bail!(
                    "Login attempts per hour must be greater than 0 when rate limiting is enabled"
                );
            }
        }

        // Validate default rooms
        for room in &self.chat.default_rooms {
            if room.id.is_empty() {
                anyhow::bail!("Room ID cannot be empty");
            }
            if room.name.is_empty() {
                anyhow::bail!("Room name cannot be empty");
            }
            if room.name.len() > self.chat.max_room_name_length {
                anyhow::bail!(
                    "Default room name '{}' exceeds maximum length of {}",
                    room.name,
                    self.chat.max_room_name_length
                );
            }
        }

        // Validate admin users
        for admin in &self.admin.users {
            if admin.username.is_empty() {
                anyhow::bail!("Admin username cannot be empty");
            }
            if admin.password.is_empty() {
                anyhow::bail!("Admin password cannot be empty");
            }
            if admin.password.len() < 8 {
                anyhow::bail!(
                    "Admin password for '{}' must be at least 8 characters",
                    admin.username
                );
            }
        }

        // Validate CORS configuration
        if !self.server.cors.allow_any_origin && self.server.cors.allowed_origins.is_empty() {
            anyhow::bail!("CORS: If allow_any_origin is false, allowed_origins must be specified");
        }

        Ok(())
    }

    /// Get the socket address for the server
    pub fn socket_addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::from((self.server.host, self.server.port))
    }

    /// Get the log filter string for tracing
    pub fn log_filter(&self) -> String {
        // If it looks like a full filter string, use it as-is
        if self.logging.level.contains('=') || self.logging.level.contains(',') {
            self.logging.level.clone()
        } else {
            // Otherwise, construct a filter string
            format!(
                "bidirectional_chat_server={},ras_jsonrpc_bidirectional_server={},{}",
                self.logging.level, self.logging.level, self.logging.level
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config {
            server: ServerConfig {
                host: default_host(),
                port: default_port(),
                cors: CorsConfig::default(),
            },
            auth: AuthConfig {
                jwt_secret: default_jwt_secret(),
                jwt_ttl_seconds: default_jwt_ttl(),
                refresh_enabled: true,
                jwt_algorithm: default_jwt_algorithm(),
            },
            chat: ChatConfig {
                data_dir: default_data_dir(),
                max_message_length: default_max_message_length(),
                max_room_name_length: default_max_room_name_length(),
                max_users_per_room: 0,
                default_rooms: default_rooms(),
                persist_messages: true,
                persist_rooms: true,
                persist_profiles: true,
            },
            logging: LoggingConfig {
                level: default_log_level(),
                format: default_log_format(),
                timestamps: true,
                target: true,
                line_numbers: true,
                thread_ids: true,
            },
            admin: AdminConfig {
                users: vec![],
                auto_create: true,
            },
            rate_limit: RateLimitConfig {
                enabled: false,
                messages_per_minute: default_messages_per_minute(),
                connections_per_ip: default_connections_per_ip(),
                login_attempts_per_hour: default_login_attempts_per_hour(),
            },
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_config() {
        let mut config = Config {
            server: ServerConfig {
                host: default_host(),
                port: 0, // Invalid
                cors: CorsConfig::default(),
            },
            auth: AuthConfig {
                jwt_secret: default_jwt_secret(),
                jwt_ttl_seconds: default_jwt_ttl(),
                refresh_enabled: true,
                jwt_algorithm: default_jwt_algorithm(),
            },
            chat: ChatConfig {
                data_dir: default_data_dir(),
                max_message_length: default_max_message_length(),
                max_room_name_length: default_max_room_name_length(),
                max_users_per_room: 0,
                default_rooms: default_rooms(),
                persist_messages: true,
                persist_rooms: true,
                persist_profiles: true,
            },
            logging: LoggingConfig {
                level: default_log_level(),
                format: default_log_format(),
                timestamps: true,
                target: true,
                line_numbers: true,
                thread_ids: true,
            },
            admin: AdminConfig {
                users: vec![],
                auto_create: true,
            },
            rate_limit: RateLimitConfig {
                enabled: false,
                messages_per_minute: default_messages_per_minute(),
                connections_per_ip: default_connections_per_ip(),
                login_attempts_per_hour: default_login_attempts_per_hour(),
            },
        };

        assert!(config.validate().is_err());

        // Fix port but break message length
        config.server.port = 3000;
        config.chat.max_message_length = 0;
        assert!(config.validate().is_err());
    }
}
