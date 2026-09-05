use ast::*;
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod ast;
mod expand;
mod parser;
mod server;

mod client;
mod openapi;
mod permissions;
mod static_hosting;

/// Macro to generate a REST service with authentication support
///
/// This macro generates a service trait and builder that integrates with axum
/// for handling REST requests with authentication and authorization.
///
/// Supports HTTP methods: GET, POST, PUT, DELETE, PATCH
/// Supports path parameters and request bodies
/// Generates OpenAPI 3.0 documents using schemars
///
/// # Auth levels
///
/// Each endpoint declares one of three auth levels:
///
/// * `UNAUTHORIZED` — public; the handler receives no caller.
/// * `OPTIONAL_AUTH` — public, but opportunistically identified: the route is
///   never rejected for auth reasons and the handler receives a
///   [`ras_auth_core::Caller`] (`Anonymous`, or `Authenticated(user)` when a
///   valid credential is present). A present-but-bad credential (invalid/expired
///   token, or a cookie that fails CSRF on an unsafe method) resolves to
///   `Anonymous` rather than rejecting.
/// * `WITH_PERMISSIONS([...])` — authenticated and gated; a missing or
///   insufficient credential is rejected before the handler runs.
///
/// # Request bodies and `Content-Type`
///
/// Endpoints that declare a body type read and JSON-decode it only **after** the
/// auth/CSRF/permission checks pass, so unauthenticated callers cannot make the
/// server buffer or parse payloads. By default a request whose `Content-Type` is
/// not `application/json` (ignoring parameters such as `; charset=utf-8`) is
/// rejected with `415 Unsupported Media Type` before the body is read. This is
/// defense-in-depth: requiring `application/json` forces a CORS preflight for
/// cross-origin requests, closing the simple-request CSRF shape (a cross-origin
/// `text/plain` POST). Set `require_json_content_type: false` at the service
/// level to accept any content type (e.g. for clients that cannot set the
/// header). A malformed body is logged (category + line/column, never the
/// rejected value) and answered with `400`; a body over the size limit is `413`,
/// distinct from an unreadable stream (`400`).
///
/// # Service options
///
/// * `body_limit: <bytes>` — maximum request body size (default 2 MiB).
/// * `require_json_content_type: <bool>` — enforce `application/json` on bodied
///   endpoints (default `true`).
/// * `serve_docs: <bool>` / `docs_path: "..."` — host the API explorer and
///   `openapi.json`.
/// * `docs_require_auth: <bool>` — when `serve_docs` is enabled, gate the docs
///   page and `openapi.json` behind authentication (any authenticated user).
///   Default `false`: docs are public, matching conventional API explorers.
/// * `feature_gated: <bool>` — wrap the generated server/client behind the
///   consumer crate's own `server`/`client` features.
///
/// # Per-endpoint options
///
/// A trailing `{ ... }` block after the response type accepts:
///
/// * `body_limit: <bytes>` — override the service body limit for this endpoint.
/// * `headers: true` — pass the request [`axum::http::HeaderMap`] to the handler
///   as an extra parameter, immediately after the caller/user and before the
///   path parameters. The map is unredacted (it contains the caller's
///   `Authorization`/`Cookie`/CSRF headers), so must not be logged or forwarded
///   verbatim; redact with
///   `ras_auth_core::redact_sensitive_headers_for_auth_transport` first.
/// * `version: "..."` / `versions: [ ... ]` — see Versioning.
///
/// # Versioning
///
/// An endpoint may serve older payload shapes at legacy paths and migrate them
/// to the canonical request/response types. Provide a canonical `version:` label
/// and one or more `versions: [ "vN" { path: ..., request: T, response: U,
/// migration: M }, ... ]` entries, where `M` implements
/// [`ras_rest_core::VersionMigration`] for both the request (legacy → canonical)
/// and the response (canonical → legacy). Each legacy path is registered as its
/// own route sharing the endpoint's auth level.
///
/// # Example
///
/// ```rust
/// use ras_rest_macro::rest_service;
/// use serde::{Deserialize, Serialize};
/// use schemars::JsonSchema;
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct UsersResponse {
///     users: Vec<()>,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct CreateUserRequest {
///     name: String,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct UserResponse {
///     id: String,
///     name: String,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct UpdateUserRequest {
///     name: String,
/// }
///
/// rest_service!({
///     service_name: UserService,
///     base_path: "/api/v1",
///     openapi: true,
///     serve_docs: true,
///     docs_path: "/docs",
///     ui_theme: "default",
///     endpoints: [
///         GET UNAUTHORIZED users() -> UsersResponse,
///         GET OPTIONAL_AUTH feed() -> UsersResponse,
///         POST WITH_PERMISSIONS(["admin"]) users(CreateUserRequest) -> UserResponse,
///         GET WITH_PERMISSIONS(["user"]) users/{id: String}() -> UserResponse,
///         PUT WITH_PERMISSIONS(["admin"]) users/{id: String}(UpdateUserRequest) -> UserResponse,
///         DELETE WITH_PERMISSIONS(["admin"]) users/{id: String}() -> (),
///     ]
/// });
///
/// # fn main() {}
/// ```
#[proc_macro]
pub fn rest_service(input: TokenStream) -> TokenStream {
    let service_definition = parse_macro_input!(input as ServiceDefinition);

    match expand::generate_service_code(service_definition) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
