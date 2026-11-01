//! The `rest_service!` procedural macro.
//!
//! One `rest_service!` definition describes a JSON REST API: each endpoint's HTTP method, auth
//! requirement, path and query parameters, body type and response type. From it the macro
//! generates the server (an `async_trait` handler trait and an axum router builder), a typed Rust
//! client, and, with `openapi: true`, functions that build an OpenAPI 3.0 document and an
//! optional API explorer.
//!
//! The generated code refers to runtime crates by name, so a crate that calls the macro also
//! depends on `ras-rest-core`, `ras-auth-core`, `async-trait`, `axum` and `axum-extra` (with its
//! `query` feature) for the server, `ras-transport-core` for the client, and `serde`,
//! `serde_json` and `schemars` in every build.
//!
//! # Cargo features
//!
//! * `server` (default): the handler trait, the router builder and the OpenAPI functions.
//! * `client`: the client and its builder, with `build_with_transport`.
//! * `reqwest` (default): `client`, plus the reqwest-backed `build()` on the client builder.
//! * `permissions`: a permission manifest built from the auth requirements, using
//!   `ras-permission-manifest`.
//!
//! The [`rest_service!` guide](https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/macros/rest-service.md)
//! is the full reference.

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

/// Generates a JSON REST service from a definition of its endpoints.
///
/// For a service named `UserService` this generates `UserServiceTrait` and `UserServiceBuilder`
/// (server), `UserServiceClient` and `UserServiceClientBuilder` (client), and, with `openapi`,
/// `generate_userservice_openapi`, `try_generate_userservice_openapi` and
/// `generate_userservice_openapi_to_file` in server builds.
///
/// # Endpoints
///
/// Each endpoint is `METHOD AUTH path ? query (Body) -> Response`, optionally preceded by a doc
/// comment and followed by an options block:
///
/// * `METHOD` is `GET`, `POST`, `PUT`, `DELETE` or `PATCH`.
/// * The path is identifiers joined by `/`, with `{name: Type}` for a path parameter.
/// * `? name: Type & other: Type` declares query parameters; `Option` ones may be left out.
/// * `(Body)` is the JSON body type; `()` means no body.
///
/// The handler name is the lower-case method followed by the path segments, with a path
/// parameter written as `by_<name>`: `GET users/{id: String}()` becomes `get_users_by_id`.
///
/// `AUTH` is one of:
///
/// * `UNAUTHORIZED`: public; the handler receives no caller.
/// * `OPTIONAL_AUTH`: public, but the handler receives a `ras_auth_core::Caller`, which is
///   `Authenticated(user)` for a valid credential and `Anonymous` otherwise. A credential that is
///   present but bad (an invalid or expired token, or a cookie that fails CSRF on an unsafe
///   method) resolves to `Anonymous`.
/// * `WITH_PERMISSIONS([...])`: the handler receives `&AuthenticatedUser`; a missing or
///   insufficient credential is rejected before the handler runs. Groups are OR-ed with `|` and
///   the permissions inside a group are AND-ed; `WITH_PERMISSIONS([])` allows any authenticated
///   caller.
///
/// Handler arguments come in this order: the caller or user, the headers when `headers: true`,
/// the path parameters, the query parameters, and the body.
///
/// # Request bodies and `Content-Type`
///
/// Endpoints that declare a body read and JSON-decode it only after the auth, CSRF and
/// permission checks pass, so unauthenticated callers cannot make the server buffer or parse
/// payloads. By default a request whose `Content-Type` is not `application/json` (ignoring
/// parameters such as `; charset=utf-8`) is rejected with `415` before the body is read. A
/// cross-origin `text/plain` POST skips the CORS preflight, so the check closes that route. A
/// malformed body is logged (category and line/column, never the value) and answered with
/// `400`; a body over the size limit is `413`, and an unreadable stream is `400`.
///
/// # Service options
///
/// `service_name` and `base_path` come first, then these optional fields, then `endpoints`:
///
/// * `openapi: true` or `openapi: { output: "path.json" }`: emit the OpenAPI functions.
/// * `serve_docs: <bool>` and `docs_path: "..."` (default `"/docs"`): serve the API explorer and
///   `openapi.json` under `base_path`. Needs `openapi`; without it the macro reports a compile
///   error at `serve_docs`.
/// * `docs_require_auth: <bool>` (default `false`): require an authenticated caller for the
///   explorer and `openapi.json`.
/// * `require_json_content_type: <bool>` (default `true`): enforce `application/json` on
///   endpoints with a body. Set it to `false` for a client that cannot set the header.
/// * `body_limit: <bytes>` (default 2 MiB): maximum request body size.
/// * `feature_gated: <bool>` (default `false`): wrap the server and client in
///   `#[cfg(feature = "server")]` and `#[cfg(feature = "client")]`, resolved against the calling
///   crate's own features, and the client's `build()` in `#[cfg(feature = "reqwest")]`.
///
/// # Endpoint options
///
/// A trailing `{ ... }` block after the response type accepts:
///
/// * `body_limit: <bytes>`: overrides the service body limit for this endpoint.
/// * `headers: true`: passes the request's `axum::http::HeaderMap` to the handler. The map is
///   unredacted (it contains the caller's `Authorization`, `Cookie` and CSRF headers), so do not
///   log or forward it as it is; `ras_auth_core::redact_sensitive_headers_for_auth_transport`
///   removes them.
/// * `examples: [...]`: request examples with their response or error, in the OpenAPI document.
/// * `version: label` and `versions: [...]`: see below.
///
/// # Versioning
///
/// An endpoint may serve older payload shapes at legacy paths and migrate them to the current
/// types. Give it a `version` label and `versions: [v1 { path: ..., body: T, response: U,
/// migration: M }, ...]`. The server generates a request struct per version,
/// `<Service><Handler><Version>Request`, holding `path`, `query` and `body`. `M` implements
/// `ras_rest_core::VersionMigration` from the legacy request struct to the current one, and
/// from the current response type to the legacy one. Each legacy path is its own route with the
/// endpoint's auth requirement.
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
