//! Parsed JSON-RPC service contract shared by parser and emitters.

use syn::{Ident, Type};

#[derive(Debug)]
pub(crate) struct ServiceDefinition {
    pub(crate) service_name: Ident,
    pub(crate) openrpc: Option<OpenRpcConfig>,
    pub(crate) explorer: Option<ExplorerConfig>,
    pub(crate) feature_gated: bool,
    /// Require an `application/json` request `Content-Type`. Defaults to `true`.
    /// Set `require_json_content_type: false` to accept any content type.
    pub(crate) require_json_content_type: bool,
    /// Maximum request body size in bytes. Defaults to 2 MiB (axum's default).
    pub(crate) body_limit: Option<usize>,
    /// Gate the explorer page and `openrpc.json` behind authentication (any
    /// authenticated user). Defaults to `false` — the explorer is public when
    /// enabled, matching conventional API-explorer behavior.
    pub(crate) docs_require_auth: bool,
    pub(crate) methods: Vec<MethodDefinition>,
}

/// Default maximum JSON body size in bytes (matches axum's default).
pub(crate) const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum OpenRpcConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
pub(crate) enum ExplorerConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
pub(crate) struct MethodDefinition {
    pub(crate) docs: Option<DocComment>,
    pub(crate) auth: AuthRequirement,
    pub(crate) name: Ident,
    pub(crate) request_type: Type,
    pub(crate) response_type: Type,
    pub(crate) version: Option<String>,
    pub(crate) wire_name: Option<String>,
    pub(crate) versions: Vec<MethodVersionDefinition>,
}

#[derive(Debug)]
pub(crate) struct MethodVersionDefinition {
    pub(crate) version: String,
    pub(crate) wire_name: String,
    pub(crate) request_type: Type,
    pub(crate) response_type: Type,
    pub(crate) migration_type: Type,
}

#[derive(Debug)]
pub(crate) struct DocComment {
    pub(crate) summary: String,
    pub(crate) description: String,
}

impl DocComment {
    pub(crate) fn from_lines(lines: Vec<String>) -> Option<Self> {
        let lines: Vec<String> = lines
            .into_iter()
            .map(|line| line.trim().to_string())
            .collect();
        let start = lines.iter().position(|line| !line.is_empty())?;
        let end = lines.iter().rposition(|line| !line.is_empty())?;
        let lines = &lines[start..=end];

        Some(Self {
            summary: lines[0].clone(),
            description: lines.join("\n"),
        })
    }
}

#[derive(Debug)]
pub(crate) enum AuthRequirement {
    Unauthorized,
    /// Public method that opportunistically identifies its caller. Never rejected
    /// for auth reasons; the handler receives a `ras_jsonrpc_core::Caller`.
    OptionalAuth,
    WithPermissions(Vec<Vec<String>>), // Vec of permission groups - OR between groups, AND within groups
}
