//! Audit events for authorization decisions.
//!
//! Events carry ids, audiences, permissions, and outcomes — never secrets,
//! proofs, or token values. The sink API is append-only.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    ServiceRegistered,
    ServiceDisabled,
    ManifestImported,
    RoleDefined,
    RoleBound,
    GrantAdded,
    GrantRevoked,
    TokenIssued,
    TokenIssuanceDenied,
    IdentityVerificationFailed,
    SigningKeyRotated,
    SigningKeyRemoved,
    PolicyLoaded,
}

/// One append-only audit record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub kind: AuditEventKind,
    /// The acting principal/service id, where applicable.
    pub actor: Option<String>,
    /// The target (audience, service id, role id, key id), where applicable.
    pub target: Option<String>,
    /// Human-readable detail. Must never contain secret or token material.
    pub detail: String,
}

impl AuditEvent {
    pub fn new(kind: AuditEventKind, detail: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            kind,
            actor: None,
            target: None,
            detail: detail.into(),
        }
    }

    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

/// Receives audit events. Implementations must treat the stream as
/// append-only; there is deliberately no removal API.
#[async_trait]
pub trait AuditSink: Send + Sync {
    async fn record(&self, event: AuditEvent);
}

/// In-memory append-only audit sink for embedded mode, tests, and examples.
#[derive(Default)]
pub struct InMemoryAuditSink {
    events: Mutex<Vec<AuditEvent>>,
}

impl InMemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of all recorded events, oldest first.
    pub async fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().await.clone()
    }
}

#[async_trait]
impl AuditSink for InMemoryAuditSink {
    async fn record(&self, event: AuditEvent) {
        self.events.lock().await.push(event);
    }
}

/// A sink that drops everything (for callers that do not want auditing).
pub struct NoopAuditSink;

#[async_trait]
impl AuditSink for NoopAuditSink {
    async fn record(&self, _event: AuditEvent) {}
}
