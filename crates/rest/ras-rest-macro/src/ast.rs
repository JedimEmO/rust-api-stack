//! Parsed REST service contract shared by the parser and emitters.

use crate::static_hosting;
use syn::{Ident, Type};

#[derive(Debug)]
pub(crate) struct ServiceDefinition {
    pub(crate) service_name: Ident,
    pub(crate) base_path: String,
    pub(crate) openapi: Option<OpenApiConfig>,
    pub(crate) static_hosting: static_hosting::StaticHostingConfig,
    pub(crate) body_limit: Option<usize>,
    pub(crate) feature_gated: bool,
    /// Require an `application/json` request `Content-Type` on every endpoint
    /// that declares a body. Defaults to `true`. Set `require_json_content_type:
    /// false` to opt out (e.g. for clients that cannot set the header).
    pub(crate) require_json_content_type: bool,
    /// Gate the generated docs page and `openapi.json` behind authentication
    /// (any authenticated user). Defaults to `false` — docs are public when
    /// `serve_docs` is enabled, matching conventional API-explorer behavior.
    pub(crate) docs_require_auth: bool,
    pub(crate) endpoints: Vec<EndpointDefinition>,
}

/// Default maximum JSON body size in bytes (matches axum's default).
pub(crate) const DEFAULT_BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum OpenApiConfig {
    Enabled,
    WithPath(String),
}

#[derive(Debug)]
pub(crate) struct EndpointDefinition {
    pub(crate) docs: Option<DocComment>,
    pub(crate) method: HttpMethod,
    pub(crate) auth: AuthRequirement,
    pub(crate) path: String,
    pub(crate) path_params: Vec<PathParam>,
    pub(crate) query_params: Vec<QueryParam>,
    pub(crate) request_type: Option<Type>,
    pub(crate) response_type: Type,
    pub(crate) handler_name: Ident,
    pub(crate) version: Option<String>,
    pub(crate) versions: Vec<EndpointVersionDefinition>,
    /// Per-endpoint request body size cap (bytes). Overrides the service-level
    /// `body_limit` for this endpoint when set.
    pub(crate) body_limit: Option<usize>,
    /// When `true`, the handler receives the request `HeaderMap` as an extra
    /// parameter (immediately after the caller/user, before path params).
    pub(crate) with_headers: bool,
}

#[derive(Debug)]
pub(crate) struct EndpointVersionDefinition {
    pub(crate) version: String,
    pub(crate) path: String,
    pub(crate) path_params: Vec<PathParam>,
    pub(crate) query_params: Vec<QueryParam>,
    pub(crate) request_type: Option<Type>,
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

#[derive(Debug, Clone)]
pub(crate) enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl HttpMethod {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PathParam {
    pub(crate) name: Ident,
    pub(crate) param_type: Type,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryParam {
    pub(crate) name: Ident,
    pub(crate) param_type: Type,
}

#[derive(Debug)]
pub(crate) enum AuthRequirement {
    Unauthorized,
    /// Public route that opportunistically identifies its caller. Never rejected
    /// for auth reasons; the handler receives a `ras_auth_core::Caller`.
    OptionalAuth,
    WithPermissions(Vec<Vec<String>>), // Vec of permission groups - OR between groups, AND within groups
}
