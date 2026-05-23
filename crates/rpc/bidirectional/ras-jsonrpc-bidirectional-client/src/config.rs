//! Configuration types for the bidirectional JSON-RPC client

use bon::Builder;
use std::collections::HashMap;
use std::time::Duration;

/// Authentication configuration for the client
#[derive(Debug, Clone, Default)]
pub enum AuthConfig {
    /// No authentication
    #[default]
    None,
    /// JWT token sent in Authorization header
    JwtHeader { token: String },
    /// JWT token sent as a connection parameter
    JwtParams { token: String },
    /// Custom headers
    CustomHeaders { headers: HashMap<String, String> },
    /// Custom connection parameters
    CustomParams { params: HashMap<String, String> },
}

/// Reconnection retry-policy configuration.
#[derive(Debug, Clone, Builder)]
pub struct ReconnectConfig {
    /// Whether retry attempts are enabled for caller-managed reconnect loops
    #[builder(default = true)]
    pub enabled: bool,

    /// Maximum number of reconnection attempts (0 = unlimited)
    #[builder(default = 10)]
    pub max_attempts: u32,

    /// Initial delay between reconnection attempts
    #[builder(default = Duration::from_secs(1))]
    pub initial_delay: Duration,

    /// Maximum delay between reconnection attempts
    #[builder(default = Duration::from_secs(30))]
    pub max_delay: Duration,

    /// Exponential backoff multiplier
    #[builder(default = 2.0)]
    pub backoff_multiplier: f64,

    /// Jitter to add to delays (0.0 = no jitter, 1.0 = up to 100% jitter)
    #[builder(default = 0.1)]
    pub jitter: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: 0.1,
        }
    }
}

impl ReconnectConfig {
    /// Calculate the delay for a given reconnection attempt
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return self.initial_delay;
        }

        let delay_secs =
            self.initial_delay.as_secs_f64() * self.backoff_multiplier.powi(attempt as i32 - 1);

        let max_delay_secs = self.max_delay.as_secs_f64();
        let delay_secs = delay_secs.min(max_delay_secs);

        // Add jitter
        let jitter_amount = delay_secs * self.jitter;
        let jittered_delay = delay_secs + (random_unit() - 0.5) * 2.0 * jitter_amount;
        let jittered_delay = jittered_delay.max(0.0);

        // Ensure the final delay doesn't exceed max_delay
        let final_delay = jittered_delay.min(max_delay_secs);

        Duration::from_secs_f64(final_delay)
    }

    /// Check if more reconnection attempts should be made
    pub fn should_attempt(&self, attempt: u32) -> bool {
        self.enabled && (self.max_attempts == 0 || attempt < self.max_attempts)
    }
}

/// Client configuration
#[derive(Debug, Clone, Builder)]
pub struct ClientConfig {
    /// WebSocket URL to connect to
    pub url: String,

    /// Authentication configuration
    #[builder(default)]
    pub auth: AuthConfig,

    /// Reconnection configuration
    #[builder(default)]
    pub reconnect: ReconnectConfig,

    /// Request timeout
    #[builder(default = Duration::from_secs(30))]
    pub request_timeout: Duration,

    /// Heartbeat/keepalive interval (None = disabled)
    pub heartbeat_interval: Option<Duration>,

    /// Maximum number of pending requests
    #[builder(default = 1000)]
    pub max_pending_requests: usize,

    /// Custom headers to send with the WebSocket connection
    #[builder(default)]
    pub custom_headers: HashMap<String, String>,

    /// Connection timeout
    #[builder(default = Duration::from_secs(10))]
    pub connection_timeout: Duration,

    /// Buffer size for incoming messages
    #[builder(default = 1024)]
    pub message_buffer_size: usize,

    /// Whether to automatically subscribe to connection events
    #[builder(default = true)]
    pub auto_subscribe_events: bool,
}

impl ClientConfig {
    /// Create a new client configuration with the given URL
    pub fn new<S: Into<String>>(url: S) -> Self {
        Self {
            url: url.into(),
            auth: AuthConfig::default(),
            reconnect: ReconnectConfig::default(),
            request_timeout: Duration::from_secs(30),
            heartbeat_interval: Some(Duration::from_secs(30)),
            max_pending_requests: 1000,
            custom_headers: HashMap::new(),
            connection_timeout: Duration::from_secs(10),
            message_buffer_size: 1024,
            auto_subscribe_events: true,
        }
    }

    /// Get the WebSocket URL with authentication parameters if needed
    pub fn get_connection_url(&self) -> String {
        match &self.auth {
            AuthConfig::JwtParams { token } => {
                let separator = if self.url.contains('?') { "&" } else { "?" };
                format!("{}{}token={}", self.url, separator, token)
            }
            AuthConfig::CustomParams { params } => {
                let separator = if self.url.contains('?') { "&" } else { "?" };
                let param_string = params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&");
                format!("{}{}{}", self.url, separator, param_string)
            }
            _ => self.url.clone(),
        }
    }

    /// Get connection headers including authentication if needed
    pub fn get_connection_headers(&self) -> HashMap<String, String> {
        let mut headers = self.custom_headers.clone();

        match &self.auth {
            AuthConfig::JwtHeader { token } => {
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
            }
            AuthConfig::CustomHeaders {
                headers: auth_headers,
            } => {
                headers.extend(auth_headers.clone());
            }
            _ => {}
        }

        headers
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate URL
        #[cfg(not(target_arch = "wasm32"))]
        {
            use url::Url;
            Url::parse(&self.url).map_err(|e| format!("Invalid URL: {}", e))?;
        }

        // Validate timeouts
        if self.request_timeout.is_zero() {
            return Err("Request timeout must be greater than zero".to_string());
        }

        if self.connection_timeout.is_zero() {
            return Err("Connection timeout must be greater than zero".to_string());
        }

        // Validate buffer size
        if self.message_buffer_size == 0 {
            return Err("Message buffer size must be greater than zero".to_string());
        }

        // Validate max pending requests
        if self.max_pending_requests == 0 {
            return Err("Max pending requests must be greater than zero".to_string());
        }

        // Validate reconnect config
        if self.reconnect.backoff_multiplier <= 0.0 {
            return Err("Backoff multiplier must be greater than zero".to_string());
        }

        if self.reconnect.jitter < 0.0 || self.reconnect.jitter > 1.0 {
            return Err("Jitter must be between 0.0 and 1.0".to_string());
        }

        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn random_unit() -> f64 {
    rand::random::<f64>()
}

#[cfg(target_arch = "wasm32")]
fn random_unit() -> f64 {
    js_sys::Math::random()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_config_default() {
        let auth = AuthConfig::default();
        assert!(matches!(auth, AuthConfig::None));
    }

    #[test]
    fn test_reconnect_config_delay_calculation() {
        let config = ReconnectConfig::default();

        let delay1 = config.calculate_delay(1);
        let delay2 = config.calculate_delay(2);

        // Second delay should be larger due to backoff
        assert!(delay2 > delay1);

        // Should not exceed max delay (now properly capped)
        let delay_large = config.calculate_delay(100);
        assert!(
            delay_large <= config.max_delay,
            "Delay {:?} exceeds max delay {:?}",
            delay_large,
            config.max_delay
        );
    }

    #[test]
    fn test_reconnect_config_should_attempt() {
        let config = ReconnectConfig {
            max_attempts: 3,
            ..ReconnectConfig::default()
        };

        assert!(config.should_attempt(0));
        assert!(config.should_attempt(2));
        assert!(!config.should_attempt(3));
        assert!(!config.should_attempt(10));
    }

    #[test]
    fn test_reconnect_config_unlimited_attempts() {
        let config = ReconnectConfig {
            max_attempts: 0,
            ..ReconnectConfig::default()
        };

        assert!(config.should_attempt(100));
        assert!(config.should_attempt(1000));
    }

    #[test]
    fn test_client_config_builder() {
        let mut config = ClientConfig::new("ws://localhost:8080");
        config.request_timeout = Duration::from_secs(60);

        assert_eq!(config.url, "ws://localhost:8080");
        assert_eq!(config.request_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_client_config_connection_url() {
        let config = ClientConfig {
            url: "ws://localhost:8080/ws".to_string(),
            auth: AuthConfig::JwtParams {
                token: "test_token".to_string(),
            },
            ..ClientConfig::new("ws://localhost:8080/ws")
        };

        let url = config.get_connection_url();
        assert_eq!(url, "ws://localhost:8080/ws?token=test_token");
    }

    #[test]
    fn test_client_config_connection_headers() {
        let config = ClientConfig {
            url: "ws://localhost:8080/ws".to_string(),
            auth: AuthConfig::JwtHeader {
                token: "test_token".to_string(),
            },
            ..ClientConfig::new("ws://localhost:8080/ws")
        };

        let headers = config.get_connection_headers();
        assert_eq!(
            headers.get("Authorization"),
            Some(&"Bearer test_token".to_string())
        );
    }

    #[test]
    fn test_client_config_validation() {
        let mut config = ClientConfig::new("ws://localhost:8080");
        assert!(config.validate().is_ok());

        config.request_timeout = Duration::from_secs(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_each_invalid_field() {
        let base = ClientConfig::new("ws://localhost:8080");

        let mut c = base.clone();
        c.request_timeout = Duration::ZERO;
        assert!(c.validate().unwrap_err().contains("Request timeout"));

        let mut c = base.clone();
        c.connection_timeout = Duration::ZERO;
        assert!(c.validate().unwrap_err().contains("Connection timeout"));

        let mut c = base.clone();
        c.message_buffer_size = 0;
        assert!(c.validate().unwrap_err().contains("Message buffer size"));

        let mut c = base.clone();
        c.max_pending_requests = 0;
        assert!(c.validate().unwrap_err().contains("Max pending requests"));

        let mut c = base.clone();
        c.reconnect.backoff_multiplier = 0.0;
        assert!(c.validate().unwrap_err().contains("Backoff multiplier"));

        let mut c = base.clone();
        c.reconnect.jitter = 1.5;
        assert!(c.validate().unwrap_err().contains("Jitter"));

        // Native build also rejects an unparseable URL.
        let mut c = base.clone();
        c.url = "not a url".to_string();
        assert!(c.validate().is_err());
    }

    #[test]
    fn connection_url_appends_amp_when_query_already_present() {
        let cfg = ClientConfig {
            auth: AuthConfig::JwtParams {
                token: "tok".into(),
            },
            ..ClientConfig::new("ws://h/ws?x=1")
        };
        assert_eq!(cfg.get_connection_url(), "ws://h/ws?x=1&token=tok");
    }

    #[test]
    fn connection_url_with_custom_params() {
        let mut params = HashMap::new();
        params.insert("foo".to_string(), "bar".to_string());
        let cfg = ClientConfig {
            auth: AuthConfig::CustomParams { params },
            ..ClientConfig::new("ws://h/ws")
        };
        let url = cfg.get_connection_url();
        assert!(url.starts_with("ws://h/ws?"));
        assert!(url.contains("foo=bar"));
    }

    #[test]
    fn connection_headers_with_custom_headers_variant() {
        let mut headers = HashMap::new();
        headers.insert("X-API-Key".to_string(), "k".to_string());
        let cfg = ClientConfig {
            auth: AuthConfig::CustomHeaders {
                headers: headers.clone(),
            },
            ..ClientConfig::new("ws://h/ws")
        };
        let h = cfg.get_connection_headers();
        assert_eq!(h.get("X-API-Key"), Some(&"k".to_string()));
    }

    #[test]
    fn connection_url_falls_through_for_no_param_auth() {
        let cfg = ClientConfig {
            auth: AuthConfig::JwtHeader {
                token: "tok".into(),
            },
            ..ClientConfig::new("ws://h/ws")
        };
        // Header-based auth must NOT mutate the URL.
        assert_eq!(cfg.get_connection_url(), "ws://h/ws");
    }

    #[test]
    fn calculate_delay_zero_attempt_returns_initial() {
        let cfg = ReconnectConfig {
            jitter: 0.0,
            ..ReconnectConfig::default()
        };
        assert_eq!(cfg.calculate_delay(0), cfg.initial_delay);
    }

    #[test]
    fn auth_config_default_via_helper() {
        // exercises the `Default` impl + AuthConfig::default branches.
        let _ = AuthConfig::default();
        let _ = ReconnectConfig::default();
    }
}
