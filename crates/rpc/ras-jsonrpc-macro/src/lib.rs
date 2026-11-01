//! The `jsonrpc_service!` procedural macro.
//!
//! One `jsonrpc_service!` definition describes a JSON-RPC 2.0 service over HTTP: each method's
//! auth requirement, request type and response type. From it the macro generates the server
//! (a handler trait and an axum router builder), a typed Rust client, and, with `openrpc: true`,
//! functions that build an OpenRPC document and an optional API explorer.
//!
//! The generated code refers to runtime crates by name, so a crate that calls the macro also
//! depends on `ras-jsonrpc-core` and `axum` for the server, `ras-transport-core` for the client,
//! and `ras-jsonrpc-types`, `serde`, `serde_json` and `schemars` in every build.
//!
//! # Cargo features
//!
//! * `server` (default): the handler trait and the router builder.
//! * `client`: the client and its builder, with `build_with_transport`.
//! * `reqwest` (default): `client`, plus the reqwest-backed `build()` on the client builder.
//! * `permissions`: a permission manifest built from the auth requirements, using
//!   `ras-permission-manifest`.
//!
//! The [`jsonrpc_service!` guide](https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/macros/jsonrpc-service.md)
//! is the full reference.

use ast::*;
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod ast;
mod expand;
mod parser;
mod server;

mod client;
mod openrpc;
mod permissions;
mod static_hosting;

/// Generates a JSON-RPC 2.0 service from a definition of its methods.
///
/// ```ignore
/// jsonrpc_service!({
///     service_name: UserService,
///     openrpc: true,
///     methods: [
///         /// Exchange credentials for a token.
///         UNAUTHORIZED sign_in(SignInRequest) -> SignInResponse,
///         OPTIONAL_AUTH feed(()) -> Feed,
///         WITH_PERMISSIONS(["user"]) get_profile(()) -> UserProfile,
///         WITH_PERMISSIONS(["admin"] | ["support", "users:write"]) disable_user(String) -> (),
///     ]
/// });
/// ```
///
/// For a service named `UserService` this generates `UserServiceTrait` and `UserServiceBuilder`
/// (server), `UserServiceClient` and `UserServiceClientBuilder` (client), and, with `openrpc`,
/// `generate_userservice_openrpc`, `try_generate_userservice_openrpc` and
/// `generate_userservice_openrpc_to_file`.
///
/// # Methods
///
/// Each method is `AUTH name(Request) -> Response`, optionally preceded by a doc comment and
/// followed by an options block. The Rust name is also the wire method name. `AUTH` is one of:
///
/// * `UNAUTHORIZED`: public; the handler receives only the request.
/// * `OPTIONAL_AUTH`: public, but the handler receives a `ras_jsonrpc_core::Caller`, which is
///   `Authenticated(user)` for a valid credential and `Anonymous` otherwise. A credential that is
///   present but invalid resolves to `Anonymous`.
/// * `WITH_PERMISSIONS([...])`: the handler receives `&AuthenticatedUser`. Groups are OR-ed with
///   `|` and the permissions inside a group are AND-ed; `WITH_PERMISSIONS([])` allows any
///   authenticated caller.
///
/// The options block `{ ... }` takes `wire: "name"`, `version: label`, `versions: [...]` for
/// legacy wire methods with a migration type, and `examples: [...]` for call examples in the
/// OpenRPC document.
///
/// # Service options
///
/// Optional fields between `service_name` and `methods`:
///
/// * `openrpc: true` or `openrpc: { output: "path.json" }`: emit the OpenRPC functions.
/// * `explorer: true` or `explorer: { path: "/docs" }`: serve the API explorer and
///   `openrpc.json` under the JSON-RPC route. Needs `openrpc`; without it the macro reports
///   a compile error at `explorer`.
/// * `docs_require_auth: <bool>` (default `false`): require an authenticated caller for the
///   explorer and `openrpc.json`. The JSON-RPC endpoint is never affected.
/// * `require_json_content_type: <bool>` (default `true`): reject a request whose
///   `Content-Type` is not `application/json` with `415` before parsing. A cross-origin
///   `text/plain` POST skips the CORS preflight, so the check closes that route. Set it to
///   `false` for a client that cannot set the header.
/// * `body_limit: <bytes>` (default 2 MiB): maximum request body size.
/// * `feature_gated: <bool>` (default `false`): wrap the server and client in
///   `#[cfg(feature = "server")]` and `#[cfg(feature = "client")]`, resolved against the calling
///   crate's own features, and the client's `build()` in `#[cfg(feature = "reqwest")]`.
///
/// # Behaviour
///
/// A handler error is sent as `-32603 "Internal error"` without detail, unless the handler
/// returns a `ras_jsonrpc_types::PublicRpcError` directly. A request without an `id` is a
/// notification and gets no JSON-RPC response. Malformed bodies and auth rejections are logged
/// through `tracing`. The builder's `build()` fails when a `WITH_PERMISSIONS` method or a gated
/// explorer exists and no auth provider was set.
#[proc_macro]
pub fn jsonrpc_service(input: TokenStream) -> TokenStream {
    let service_definition = parse_macro_input!(input as ServiceDefinition);

    match expand::generate_service_code(service_definition) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
