use super::Client;
use crate::{
    config::{AuthConfig, ClientConfig, ReconnectConfig},
    error::ClientResult,
};
use std::{collections::HashMap, time::Duration};

/// Builder for creating a client with configuration
pub struct ClientBuilder {
    /// WebSocket URL to connect to
    url: String,

    /// JWT token for authentication
    jwt_token: Option<String>,

    /// Custom headers
    custom_headers: HashMap<String, String>,

    /// Request timeout
    request_timeout: Duration,

    /// Reconnection configuration
    reconnect_config: Option<ReconnectConfig>,

    /// Heartbeat interval
    heartbeat_interval: Option<Duration>,

    /// Connection timeout
    connection_timeout: Duration,

    /// Auto-connect after building
    auto_connect: bool,
}

impl ClientBuilder {
    /// Create a new client builder with the given URL
    pub fn new<S: Into<String>>(url: S) -> Self {
        Self {
            url: url.into(),
            jwt_token: None,
            custom_headers: HashMap::new(),
            request_timeout: Duration::from_secs(30),
            reconnect_config: None,
            heartbeat_interval: Some(Duration::from_secs(30)),
            connection_timeout: Duration::from_secs(10),
            auto_connect: false,
        }
    }

    /// Set JWT token for authentication
    pub fn with_jwt_token(mut self, token: String) -> Self {
        self.jwt_token = Some(token);
        self
    }

    /// No-op kept for source compatibility.
    ///
    /// Tokens are always sent out-of-URL: in the `Authorization` header on
    /// native targets and as the `token.<jwt>` subprotocol in browsers. The
    /// query-string transport was removed because URLs leak into logs and the
    /// bundled server never accepted it.
    #[deprecated(note = "tokens are never sent in the URL; this flag has no effect")]
    pub fn with_jwt_in_header(self, _in_header: bool) -> Self {
        self
    }

    /// Add a custom header
    pub fn with_header<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.custom_headers.insert(key.into(), value.into());
        self
    }

    /// Set request timeout
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Set reconnection configuration
    pub fn with_reconnect_config(mut self, config: ReconnectConfig) -> Self {
        self.reconnect_config = Some(config);
        self
    }

    /// Set heartbeat interval
    pub fn with_heartbeat_interval(mut self, interval: Option<Duration>) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    /// Set connection timeout
    pub fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    /// Enable auto-connect after building
    pub fn with_auto_connect(mut self, auto_connect: bool) -> Self {
        self.auto_connect = auto_connect;
        self
    }

    /// Build the client
    pub async fn build(self) -> ClientResult<Client> {
        let auth = match self.jwt_token {
            Some(token) => AuthConfig::JwtHeader { token },
            None => AuthConfig::None,
        };

        let config = ClientConfig {
            url: self.url,
            auth,
            reconnect: self.reconnect_config.unwrap_or_default(),
            request_timeout: self.request_timeout,
            heartbeat_interval: self.heartbeat_interval,
            max_pending_requests: 1000,
            custom_headers: self.custom_headers,
            connection_timeout: self.connection_timeout,
            message_buffer_size: 1024,
            auto_subscribe_events: true,
        };

        let client = Client::new(config).await?;

        if self.auto_connect {
            client.connect().await?;
        }

        Ok(client)
    }
}
