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

/// Macro to generate a JSON-RPC service with authentication support
///
/// This macro generates a service trait and builder that integrates with axum
/// for handling JSON-RPC requests with authentication and authorization.
///
/// Each method declares one of three auth levels:
///
/// * `UNAUTHORIZED` — public; the handler receives no caller.
/// * `OPTIONAL_AUTH` — public, but opportunistically identified: never rejected
///   for auth reasons, the handler receives a `ras_jsonrpc_core::Caller`
///   (`Anonymous`, or `Authenticated(user)` for a valid credential). A
///   present-but-bad credential downgrades to `Anonymous`.
/// * `WITH_PERMISSIONS([...])` — authenticated and gated.
///
/// ```ignore
/// jsonrpc_service!({
///     service_name: ApiService,
///     methods: [
///         UNAUTHORIZED register(UserRequest) -> UserResponse,
///         OPTIONAL_AUTH feed(FeedRequest) -> FeedResponse,
///         WITH_PERMISSIONS(["user.read"]) get_profile(()) -> UserResponse,
///     ]
/// });
/// ```
///
/// # Service options
///
/// Optional fields alongside `service_name` / `methods`:
///
/// * `require_json_content_type: <bool>` (default `true`) — reject a request
///   whose `Content-Type` is not `application/json` with `415` before parsing.
///   Requiring `application/json` forces a CORS preflight for cross-origin
///   requests, closing the simple-request CSRF shape. Set to `false` to accept
///   any content type (e.g. for a device client that cannot set the header).
/// * `body_limit: <bytes>` (default 2 MiB) — maximum request body size.
/// * `openrpc: true` / `explorer: true` — emit the OpenRPC document and host the
///   API explorer.
/// * `docs_require_auth: <bool>` (default `false`) — when the explorer is
///   enabled, gate the explorer page and `openrpc.json` behind authentication
///   (any authenticated user). Default is public, matching conventional API
///   explorers; the RPC endpoint itself is never gated by this option.
/// * `feature_gated: <bool>` — wrap the server/client in the consumer crate's own
///   `server`/`client` features.
///
/// A malformed JSON body and authentication/authorization rejections are logged
/// server-side (via `tracing`); a service with any `WITH_PERMISSIONS` method (or
/// a gated explorer) that is built without an auth provider fails `build()`.
///
/// See the tests for further usage examples.
#[proc_macro]
pub fn jsonrpc_service(input: TokenStream) -> TokenStream {
    let service_definition = parse_macro_input!(input as ServiceDefinition);

    match expand::generate_service_code(service_definition) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
