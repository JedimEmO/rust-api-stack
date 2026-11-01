//! Configuration types for the bidirectional JSON-RPC client

use bon::Builder;
use std::collections::HashMap;
use std::time::Duration;

/// Authentication configuration for the client.
///
/// `Debug` prints header and parameter names but never their values or the
/// token, so a logged configuration leaks no credential.
#[derive(Clone, Default)]
pub enum AuthConfig {
    /// No authentication
    #[default]
    None,
    /// JWT token sent in the `Authorization` header (native) or as the
    /// `token.<jwt>` WebSocket subprotocol (browsers, which cannot set
    /// headers). Tokens are never placed in the URL: URLs land in proxy logs,
    /// browser history and tracing spans.
    JwtHeader { token: String },
    /// Custom headers
    CustomHeaders { headers: HashMap<String, String> },
    /// Custom connection parameters (percent-encoded into the query string).
    /// Not for credentials; see `JwtHeader`.
    CustomParams { params: HashMap<String, String> },
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::JwtHeader { .. } => f
                .debug_struct("JwtHeader")
                .field("token", &REDACTED)
                .finish(),
            Self::CustomHeaders { headers } => f
                .debug_struct("CustomHeaders")
                .field("headers", &RedactedValues(headers))
                .finish(),
            Self::CustomParams { params } => f
                .debug_struct("CustomParams")
                .field("params", &RedactedValues(params))
                .finish(),
        }
    }
}

const REDACTED: Redacted = Redacted;

/// Prints `<redacted>` in place of a secret.
struct Redacted;

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Prints a map's keys, sorted, with every value redacted.
struct RedactedValues<'a>(&'a HashMap<String, String>);

impl std::fmt::Debug for RedactedValues<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<_> = self.0.keys().collect();
        keys.sort();
        f.debug_map()
            .entries(keys.into_iter().map(|key| (key, &REDACTED)))
            .finish()
    }
}

/// Subprotocol the bundled server selects on upgrade.
pub const WS_SUBPROTOCOL: &str = "ras-jsonrpc";
/// Prefix of the pseudo-subprotocol that carries a bearer token from browsers.
pub const WS_TOKEN_SUBPROTOCOL_PREFIX: &str = "token.";

/// Percent-encode a query key or value (RFC 3986 unreserved set kept as-is).
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
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

/// Client configuration.
///
/// `Debug` redacts the token and the values of custom headers, which often
/// carry credentials too.
#[derive(Clone, Builder)]
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

    /// Capacity of the outgoing message queue, and the most received
    /// messages the browser transport queues before it disconnects
    #[builder(default = 1024)]
    pub message_buffer_size: usize,

    /// Whether to automatically subscribe to connection events
    #[builder(default = true)]
    pub auto_subscribe_events: bool,
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("url", &self.url)
            .field("auth", &self.auth)
            .field("reconnect", &self.reconnect)
            .field("request_timeout", &self.request_timeout)
            .field("heartbeat_interval", &self.heartbeat_interval)
            .field("max_pending_requests", &self.max_pending_requests)
            .field("custom_headers", &RedactedValues(&self.custom_headers))
            .field("connection_timeout", &self.connection_timeout)
            .field("message_buffer_size", &self.message_buffer_size)
            .field("auto_subscribe_events", &self.auto_subscribe_events)
            .finish()
    }
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

    /// Get the WebSocket URL with custom connection parameters if configured
    pub fn get_connection_url(&self) -> String {
        match &self.auth {
            AuthConfig::CustomParams { params } => {
                let separator = if self.url.contains('?') { "&" } else { "?" };
                let mut entries: Vec<_> = params.iter().collect();
                entries.sort();
                let param_string = entries
                    .iter()
                    .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
                    .collect::<Vec<_>>()
                    .join("&");
                format!("{}{}{}", self.url, separator, param_string)
            }
            _ => self.url.clone(),
        }
    }

    /// Subprotocols to offer on upgrade.
    ///
    /// Always offers [`WS_SUBPROTOCOL`]; with `JwtHeader` auth it also offers
    /// `token.<jwt>`, the only way a browser can present a bearer token. The
    /// server selects `ras-jsonrpc`, so the token is not echoed back.
    pub fn get_subprotocols(&self) -> Vec<String> {
        let mut protocols = vec![WS_SUBPROTOCOL.to_string()];
        if let AuthConfig::JwtHeader { token } = &self.auth {
            protocols.push(format!("{WS_TOKEN_SUBPROTOCOL_PREFIX}{token}"));
        }
        protocols
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

        // Jitter must not exceed the maximum delay.
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
    fn c1_jwt_never_appears_in_connection_url() {
        let config = ClientConfig {
            auth: AuthConfig::JwtHeader {
                token: "test_token".to_string(),
            },
            ..ClientConfig::new("ws://localhost:8080/ws")
        };

        assert_eq!(config.get_connection_url(), "ws://localhost:8080/ws");
        assert_eq!(
            config.get_subprotocols(),
            vec!["ras-jsonrpc".to_string(), "token.test_token".to_string()]
        );
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
        let mut params = HashMap::new();
        params.insert("foo".to_string(), "bar".to_string());
        let cfg = ClientConfig {
            auth: AuthConfig::CustomParams { params },
            ..ClientConfig::new("ws://h/ws?x=1")
        };
        assert_eq!(cfg.get_connection_url(), "ws://h/ws?x=1&foo=bar");
    }

    #[test]
    fn c2_custom_params_are_percent_encoded() {
        let mut params = HashMap::new();
        params.insert("a b".to_string(), "x&y=z/é".to_string());
        params.insert("plain".to_string(), "ok".to_string());
        let cfg = ClientConfig {
            auth: AuthConfig::CustomParams { params },
            ..ClientConfig::new("ws://h/ws")
        };
        assert_eq!(
            cfg.get_connection_url(),
            "ws://h/ws?a%20b=x%26y%3Dz%2F%C3%A9&plain=ok"
        );
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
    fn client_configuration_debug_must_redact_tokens() {
        let mut config = ClientConfig::new("wss://api.example/ws");
        config.auth = AuthConfig::JwtHeader {
            token: "review-sensitive-token".into(),
        };
        config
            .custom_headers
            .insert("X-Api-Key".into(), "header-secret".into());
        let printed = format!("{config:?}");
        assert!(!printed.contains("review-sensitive-token"));
        assert!(!printed.contains("header-secret"));
        assert!(
            printed.contains("X-Api-Key"),
            "names stay visible: {printed}"
        );

        let headers = AuthConfig::CustomHeaders {
            headers: HashMap::from([("Authorization".into(), "Bearer custom-secret".into())]),
        };
        let params = AuthConfig::CustomParams {
            params: HashMap::from([("key".into(), "param-secret".into())]),
        };
        assert!(!format!("{headers:?}").contains("custom-secret"));
        assert!(!format!("{params:?}").contains("param-secret"));
    }

    #[test]
    fn auth_config_default_via_helper() {
        // exercises the `Default` impl + AuthConfig::default branches.
        let _ = AuthConfig::default();
        let _ = ReconnectConfig::default();
    }
}
