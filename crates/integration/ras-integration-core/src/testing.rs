//! Test doubles for the token framework.
//!
//! Shipped in the crate proper (not behind `cfg(test)`) so downstream crates
//! and applications can exercise their real service code paths against fakes
//! instead of mocking everything away.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::IntegrationError;
use crate::types::{TokenFamily, TokenLease, TokenRequest, TokenSource};

type Responder =
    dyn Fn(&TokenRequest) -> Result<TokenLease, IntegrationError> + Send + Sync + 'static;

/// A scriptable [`TokenSource`] that records every request it receives.
pub struct FakeTokenSource {
    family: TokenFamily,
    responder: Box<Responder>,
    delay: Option<std::time::Duration>,
    calls: AtomicUsize,
    requests: Mutex<Vec<TokenRequest>>,
}

impl FakeTokenSource {
    /// A fake producing responses from `responder`.
    pub fn new(
        family: TokenFamily,
        responder: impl Fn(&TokenRequest) -> Result<TokenLease, IntegrationError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            family,
            responder: Box::new(responder),
            delay: None,
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Sleep before responding — lets tests overlap concurrent requests to
    /// exercise refresh deduplication.
    pub fn with_delay(mut self, delay: std::time::Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// How many times `issue_token` ran.
    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// All requests received so far.
    pub async fn requests(&self) -> Vec<TokenRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl TokenSource for FakeTokenSource {
    fn family(&self) -> TokenFamily {
        self.family
    }

    async fn issue_token(&self, request: &TokenRequest) -> Result<TokenLease, IntegrationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().await.push(request.clone());
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        (self.responder)(request)
    }
}

/// Convenience constructor: a fake source minting `token-<n>` leases that
/// expire `ttl_secs` from issuance.
pub fn counting_source(family: TokenFamily, ttl_secs: i64) -> Arc<FakeTokenSource> {
    let counter = AtomicUsize::new(0);
    Arc::new(FakeTokenSource::new(family, move |request| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        Ok(TokenLease {
            access_token: crate::SecretString::new(format!("token-{n}")),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::seconds(ttl_secs)),
            scopes: request.scopes.clone(),
        })
    }))
}
