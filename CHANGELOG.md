# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Fixed - 2026-09-05 (audit remediation, fourth review pass)
- **Subscription limits are enforced by the server, independent of the manager** (`ras-jsonrpc-bidirectional-server`). The service now owns a `SubscriptionAccounting` counter shared by all of its connections (`WebSocketService::subscription_accounting`, a required method). After every `handle_subscribe`, the handler reconciles the context against the configured limits itself: topic length and the per-connection cap from the context, the global cap by atomic reservation on the shared counter. Only accepted topics are handed to the manager, so a permissive custom `ConnectionManager` supplied via `build_with_manager`, a `DefaultConnectionManager::new()` built with different limits, or a greedy custom `MessageHandler` writing straight into the context can no longer exceed the service's caps. Slots are released on unsubscribe, on re-validation drops, and on disconnect. `DefaultConnectionManager` keeps its own enforcement as a second line. Covered by an integration test that plugs a fully permissive custom manager and a greedy handler into `build_with_manager`; it fails with the server-side check removed.

### Fixed - 2026-09-05 (audit remediation, third review pass)
- **Subscription limits are a manager invariant** (`ras-jsonrpc-bidirectional-server`, `-types`). `DefaultConnectionManager::add_subscription` now enforces topic length, the per-connection cap and the global cap itself, reserving the global slot with an atomic counter, and returns the new `BidirectionalError::SubscriptionLimitReached` (or `InvalidTopic`) when a cap would be exceeded. The handler's pre-check is only a fast path for a friendly error response; after any `handle_subscribe`, including a custom one that writes straight to the connection context, the handler mirrors into the manager and rolls back whatever the manager refuses. `DefaultConnectionManager::with_subscription_limits` sets the caps; both builders pass their `subscription_limits` through. Custom `ConnectionManager` implementations must enforce their own caps.
- **Subscription state and index are updated as one unit** (`ras-jsonrpc-bidirectional-server`). `add_subscription` and `remove_subscription` hold the connection's entry guard while touching both the per-connection state and the topic index, so a concurrent add/remove of the same pair can no longer leave a stale index entry (and its accounting) behind. Covered by a 500-round interleaving test that fails against the previous ordering.
- **Connection admission has no advisory fallback** (`ras-jsonrpc-bidirectional-server`, breaking for custom `WebSocketService` implementations). `WebSocketService::connection_permits` is now a required method; admission takes a permit from it or runs unbounded when it returns `None`. The checked-then-added path is gone. `max_connections` is informational.
- `sanitize_log_detail` keeps its 256-byte contract: the ellipsis marker is counted inside the budget (`ras-auth-core`).

### Fixed - 2026-09-05 (audit remediation, second review pass)
- **Revocation race closed with an egress gate** (`ras-jsonrpc-bidirectional-server`). A `broadcast_to_topic` that snapshotted the topic index in the window between a re-validation's permission change and the subscription's removal could still deliver. Every message the manager routes on a topic is now tagged with that topic in the connection's outbound queue (`OutboundMessage`, `ChannelMessageSender::send_on_topic`), and the handler loop re-checks the subscription immediately before writing to the socket, dropping the message if the subscription is gone. Re-authorization removes the subscription from the connection context before the manager index, so the gate is authoritative from the first instant. `ChannelMessageSender::new` now takes an `mpsc::Sender<OutboundMessage>`; code that reads the raw queue must unwrap `.message`.
- **Connection admission is atomic** (`ras-jsonrpc-bidirectional-server`). `BuiltWebSocketService` holds a `tokio::sync::Semaphore` sized to `max_connections`; a permit is taken before the upgrade and held for the connection's lifetime, so concurrent upgrades cannot overshoot the cap. Custom `WebSocketService` implementations can expose the same via the new `connection_permits()` method; without it the previous advisory check applies.
- **Zero durations no longer panic** (`ras-jsonrpc-bidirectional-server`). A zero `auth_revalidation_interval` falls back to the 30 s default with a warning; a zero keepalive ping interval or idle timeout disables that half of the keepalive with a warning. Previously either reached `tokio::time::interval_at` and panicked the connection task.
- **Rejection detail is sanitized before logging** (`ras-auth-core`, `ras-rest-macro`, `ras-file-macro`). Generated handlers pass extractor rejection text through the new `ras_auth_core::sanitize_log_detail` (re-exported from `ras-rest-core` and `ras-file-core`), which replaces control characters and truncates to 256 bytes, so a crafted path or query value cannot inject log lines or amplify log volume.
- `chacha20` refreshed from the yanked 0.10.1 to 0.10.2 in `Cargo.lock` (stale entry; nothing in the workspace depended on it).

### Changed - 2026-09-05 (audit remediation, second review pass)
- **`max_connections` is bounded by default** (`ras-jsonrpc-bidirectional-server`, breaking for deployments that relied on unbounded). `WebSocketServiceBuilder` and the generated `<Service>Builder` default to `DEFAULT_MAX_CONNECTIONS` (10 000); pass `max_connections(None)` explicitly to lift the cap. The generated builder also gained `max_message_size`, `subscription_limits`, `keepalive` and `on_permission_change` passthroughs, so every hardening knob is reachable without dropping to the low-level builder.
- **Global subscription cap** (`ras-jsonrpc-bidirectional-server`, `-types`). `SubscriptionLimits::max_total_subscriptions` (default 100 000, `0` disables) bounds (connection, topic) pairs across the whole manager, so many connections cannot multiply the per-connection allowance. `ConnectionManager` gained a defaulted `total_subscription_count()` (returns 0, disabling the cap, unless the manager tracks it; `DefaultConnectionManager` tracks it exactly with an atomic counter).

### Security - 2026-09-05 (audit remediation, one PR)
Remediates every finding from the September 2026 security review plus the follow-up gap sweep: 24 issues across the WebSocket, session, identity and auth-core crates, each with a regression test named by its ID (W1–W5, C1–C2, S1–S5, I1–I9, A1–A2, F1–F2). Version bumps in this set:

| Crate | From | To | Why |
|---|---|---|---|
| `ras-auth-core` | 0.2.0 | 0.3.0 | `AuthError` no longer `Serialize`; CSRF constructors renamed (A1, A2) |
| `ras-identity-session` | 0.3.0 | 0.4.0 | `iss`/`aud` required by default; `SessionConfig::new` signature (S2) |
| `ras-identity-local` | 0.2.1 | 0.3.0 | `password_hash` not serialized; `LocalAuthPayload` changes (I1, I2) |
| `ras-identity-oauth2` | 0.2.0 | 0.3.0 | secret not serialized; `code` optional; https enforced; `add_provider` fallible (I1, I5, I9) |
| `ras-jsonrpc-bidirectional-server` | 0.2.0 | 0.3.0 | `AuthRevalidation` gained a field; new service options (W1–W4) |
| `ras-jsonrpc-bidirectional-client` | 0.2.0 | 0.3.0 | `AuthConfig::JwtParams` removed (C1) |
| `ras-jsonrpc-bidirectional-macro` | 0.2.0 | 0.2.1 | generated handler routes permissions through the provider (W5) |
| `ras-file-core` | 0.2.0 | 0.2.1 | `sanitize_filename`, `attachment()` encoding (F1) |
| `ras-file-macro` | 0.2.0 | 0.2.1 | filename sanitization, generic rejection bodies (F1, F2) |
| `ras-rest-macro` | 0.3.0 | 0.3.1 | generic path/query rejection bodies (F2) |

Dependent crates and examples had their path-dependency version specs updated to match.


### Fixed - 2026-09-05 (WebSocket hardening — `ras-jsonrpc-bidirectional-server`, `-macro`, `-client`)
- **W1: subscriptions no longer survive permission revocation** (`ras-jsonrpc-bidirectional-server`). On every successful credential re-validation the handler re-runs `authorize_subscribe` for each held topic against the refreshed user and drops the ones no longer authorized, in both the connection context and the manager's topic index. Previously only the cached user was refreshed, so a downgraded connection kept receiving topic broadcasts until it disconnected. The new `PermissionChangePolicy` (`WebSocketService::on_permission_change`, builder field `on_permission_change`) selects `DropSubscriptions` (default) or `Close`, which closes the socket whenever the permission set changes so the client must re-authenticate.
- **Subscriptions made through the default `handle_subscribe` now reach `broadcast_to_topic`.** The handler loop mirrors subscribe/unsubscribe changes from the connection context into the connection manager's topic index; previously the two stores were never reconciled, so topics accepted by `authorize_subscribe` were invisible to manager-driven broadcasts.
- **W2: the inbound message limit is enforced at the transport** (`ras-jsonrpc-bidirectional-server`). `handle_upgrade` now sets `max_message_size` and `max_frame_size` on the Axum upgrade from `WebSocketService::max_message_size()`. Previously the 1 MiB check ran only after tungstenite had buffered the whole frame under its 64 MiB default, so any client could force 64 MiB allocations per message.
- **W5: WebSocket permission checks route through `AuthProvider::check_permissions`** (`ras-jsonrpc-bidirectional-macro`). The generated handler now carries an optional `Arc<dyn AuthProvider>` (`with_auth_provider`, set automatically by the generated builder) and uses `ras_auth_core::check_permission_groups`, so providers with wildcard, hierarchical or dynamic permission semantics behave identically over WebSocket, REST and JSON-RPC. Handlers built by hand without a provider fall back to plain set membership as before. The insufficient-permissions error now uses `JsonRpcError::insufficient_permissions` (code from `error_codes`, `required` only in `data`).
- **Browser clients can now authenticate** (`ras-jsonrpc-bidirectional-client`, `-server`). The WASM transport never sent a token at all, and the server never selected a subprotocol, so a browser offering `token.<jwt>` had its upgrade rejected. The client now offers `ras-jsonrpc` plus `token.<jwt>` (`ClientConfig::get_subprotocols`), the server parses comma-separated `Sec-WebSocket-Protocol` lists and selects `ras-jsonrpc` (`WS_SUBPROTOCOL`), so the token is read but never echoed in the response.

### Added - 2026-09-05 (WebSocket hardening)
- **W3: `SubscriptionLimits`** (`ras-jsonrpc-bidirectional-server`) — `WebSocketService::subscription_limits()` / builder field `subscription_limits`. Defaults: 64 topics per message, 256 per connection, 256-byte topic names. An over-limit `Subscribe` is answered with an invalid-params error and leaves the connection open; the service's `handle_subscribe` never sees it.
- **W4: `KeepaliveConfig`** (`ras-jsonrpc-bidirectional-server`) — `WebSocketService::keepalive()` / builder field `keepalive`. The server pings every 30 s and closes a connection that produces no inbound frame for 90 s (browsers and tungstenite answer pings automatically). Either half can be disabled with `None`. `max_connections` stays unbounded by default; production deployments should set it.
- `WS_SUBPROTOCOL` / `WS_TOKEN_SUBPROTOCOL_PREFIX` constants exported from `ras-jsonrpc-bidirectional-server` and `-client`.

### Changed - 2026-09-05 (WebSocket hardening — breaking, `ras-jsonrpc-bidirectional-client` 0.3.0)
- **C1: `AuthConfig::JwtParams` removed.** It placed the token in the URL query string, where it enters proxy logs, browser history and tracing spans, and the bundled server never read it from there. Use `AuthConfig::JwtHeader` (header on native, subprotocol in browsers). `ClientBuilder::with_jwt_in_header` is now a deprecated no-op.
- **C2: `AuthConfig::CustomParams` are percent-encoded** and emitted in sorted key order. Previously keys and values were concatenated raw.
- `AuthRevalidation` gained the `on_permission_change` field (`ras-jsonrpc-bidirectional-server` 0.3.0); `WebSocketHandler` gained `with_connection_manager`, `with_subscription_limits`, `with_keepalive`.

### Changed - 2026-09-05 (`ras-identity-session` hardening, S1–S5)
- **`iss`/`aud` are now required by default (S2). Breaking.** `SessionConfig` gained `require_iss_aud: bool` (default `true`); `SessionConfig::validate` (and therefore `SessionService::new`) fails when either `iss` or `aud` is `None`. `SessionConfig::new` now takes the issuer and audience: `SessionConfig::new(secret, iss, aud)`. Single-service deployments that never share a secret can opt out with `SessionConfig::new_unscoped(secret)` or `.allow_unscoped_tokens()`. Struct-literal callers must add the new `require_iss_aud` and `max_sessions_per_user` fields. Examples (`bidirectional-chat`, `oauth2-demo`, `google_oauth2`) and the identity READMEs now set a real issuer/audience.
- **Stricter `jwt_secret` validation (S3).** In addition to the 32-byte minimum, a secret is rejected when it contains fewer than 10 distinct byte values, a run of 8 or more identical bytes, or (case-insensitive substring) any of `change-me`, `changeme`, `secret`, `password`, `example`, `placeholder`, `test-secret`, `dev-secret`, `insecure`, `12345678`, `abcdefgh`, `your-secret`. Placeholder secrets in the example configs, `.env.example`, READMEs and test fixtures were replaced with random hex values..
- **`begin_session`/`verify_session` no longer sweep the session store inline (S1).** The previous implementation took the `active_sessions` write lock and walked the whole map on every call, before the token was even decoded. Expired entries are now pruned lazily at most once per 60 s (a cheap atomic check on the hot path) and by `start_cleanup_task`, which should be started whenever `enforce_active_sessions` is on.

### Added - 2026-09-05 (`ras-identity-session` hardening)
- **`nbf` claim (S4).** `JwtClaims` gained an optional `nbf: Option<i64>` (serde default, omitted when `None`). `verify_session` rejects a token whose `iat` or `nbf` is more than `CLOCK_SKEW_LEEWAY_SECS` (60 s) in the future with `SessionError::InvalidSession`.
- **Per-user session cap (S5).** `SessionConfig::max_sessions_per_user` (default `DEFAULT_MAX_SESSIONS_PER_USER` = 32, builder `with_max_sessions_per_user`, must be ≥ 1). When `enforce_active_sessions` is on and a user already holds that many sessions, `begin_session` evicts their oldest sessions (by `iat`) before inserting the new one, so a credential-stuffing loop cannot grow the in-memory store without bound.
- `SessionConfig::new_unscoped`, `SessionConfig::allow_unscoped_tokens`, `SessionConfig::with_max_sessions_per_user`, and the `CLOCK_SKEW_LEEWAY_SECS` / `DEFAULT_MAX_SESSIONS_PER_USER` constants. `Debug` for `SessionConfig` shows the two new fields (secret still redacted).

### Added - 2026-09-05 (identity provider hardening I1–I9)
- **`OAuth2ProviderConfig::metadata_claims: Vec<String>` (I8).** Allow-list of additional userinfo claims copied into `VerifiedIdentity.metadata` (and therefore the session JWT). Defaults to empty via `#[serde(default)]`; previously *every* extra claim the IdP returned was merged into metadata.
- **`OAuth2ProviderConfig::allow_insecure_endpoints: bool` and `OAuth2ProviderConfig::validate()` (I9).** Authorization/token/userinfo endpoints must be `https://`; `validate()` rejects anything else with `OAuth2Error::ConfigError` unless the flag (serde default `false`) is set. Only enable it for a local mock IdP.
- **`OAuth2Error::ProviderDenied { error }` and `OAuth2Error::InvalidCallback` (I5).** A callback carrying `error=…` (e.g. `access_denied`) now maps to `ProviderDenied` with only the standardized error code; `error_description` is logged at `warn` server-side and never echoed. A callback with neither `code` nor `error` returns `InvalidCallback`.
- **`ras_identity_local::MAX_PASSWORD_BYTES` (1024) and `LocalUserError::{PasswordTooLong, HashTaskFailed}` (I4).** `add_user` rejects longer passwords with `PasswordTooLong`; `verify` rejects them with the usual `InvalidCredentials` so nothing about the account is revealed.
- `InMemoryStateStore::len()` / `is_empty()` accessors.
- `ras-identity-oauth2` now depends on `subtle` (workspace dep).

### Changed - 2026-09-05 (identity provider hardening I1–I9)
- **`LocalUser.password_hash` is no longer serialized (I1a, breaking).** The field carries `#[serde(skip_serializing)]`; `Serialize` output omits it entirely. `Deserialize` still requires it. Anything that persisted `LocalUser` via serde must now store the hash separately.
- **`OAuth2ProviderConfig.client_secret` is no longer serialized (I1b, breaking).** Same treatment: dumped configs never contain the secret; deserialization still requires it.
- **`LocalAuthPayload` no longer derives `Serialize` and has a redacting `Debug` (I2, breaking).** `{:?}` prints `password: "[REDACTED]"`. Nothing in the workspace serialized the payload; build the login JSON with `serde_json::json!` instead.
- **Argon2 runs on the blocking pool and outside the users lock (I4).** `add_user` and `verify` clone the stored hash out of the `RwLock` and run `hash_password` / `verify_password` in `tokio::task::spawn_blocking`; the read lock is no longer held for the duration of a hash and the async executor is no longer stalled. The concurrency semaphore and the sentinel-hash timing behaviour for unknown users are unchanged.
- **`InMemoryStateStore` evicts instead of refusing at capacity, and sweeps at most every 10 s (I3).** `store` no longer returns `TooManyPendingFlows` when `max_states` is reached: it force-sweeps expired flows and, if still full, evicts the pending flow closest to expiry. The opportunistic expired-state sweep is rate-limited to once per 10 seconds instead of an O(n) `retain` on every call (`cleanup_expired` still sweeps unconditionally). Production deployments should rate-limit flow starts at the edge; see the type docs. `OAuth2Error::TooManyPendingFlows` is now `#[deprecated]` (kept for custom `OAuth2StateStore` implementations).
- **`AuthorizationResponse.code` and `OAuth2AuthPayload::Callback { code }` are `Option<String>` (I5, breaking).** A legitimate `error=access_denied` redirect carries no code and is no longer an `InvalidPayload`.
- **`OAuth2Error::HttpError` displays a fixed `"upstream request failed"` (I6).** The underlying `reqwest::Error` (which embeds the request URL) is logged at `warn` at the transport and remains reachable via `source()`, but no longer reaches `IdentityError::ProviderError` strings. Undecodable token/userinfo responses likewise log the reqwest error and surface a fixed message.
- **OAuth2 session-binding comparison is constant-time (I7).** `handle_callback` compares the stored binding against the callback value with `subtle::ConstantTimeEq`; semantics (missing or mismatched value → `InvalidState`, unbound flow ignores the callback value) are unchanged.
- **Provider construction validates every provider config (I9, breaking).** `OAuth2Provider::try_new` returns `ConfigError` for a non-`https://` endpoint (unless `allow_insecure_endpoints`), `OAuth2Provider::new` panics with `invalid OAuth2 configuration` (consistent with the existing `OAuth2Client::new` panic), and **`OAuth2Provider::add_provider` now returns `OAuth2Result<()>`**. The in-crate mock-IdP tests set `allow_insecure_endpoints: true`.
- `OAuth2ProviderConfig` gained two fields, so struct literals must add `metadata_claims: Vec::new()` and `allow_insecure_endpoints: false` (updated: `examples/oauth2-demo`, the crate's `google_oauth2` example and README).

### Changed - 2026-09-05 (security hardening — `ras-auth-core`, `ras-file-core`, `ras-file-macro`, `ras-rest-macro`)
- **Weak CSRF modes are renamed `dangerous_*` and warn when paired with cookie auth (A1).** `ras-auth-core`: `CsrfConfig::header_presence_only` → `CsrfConfig::dangerous_header_presence_only`, `CsrfConfig::with_expected_value` → `CsrfConfig::dangerous_static_value`. Neither mode binds the token to the session (presence-only relies entirely on restrictive credentialed CORS; a static value is a shared process-wide secret). The old names remain as `#[deprecated]` thin wrappers for one release. `AuthTransportConfig::with_cookie` / `with_csrf` now emit a `tracing::warn!` when cookie auth is combined with either mode, and `AuthTransportConfig::validate` warns as a fallback for struct-literal configs (rate-limited to once per distinct weak config per process, since `validate` runs on every request). New `CsrfConfig::dangerous_mode()` reports which weak mode, if any, is active. `ras-auth-core` gains a direct `tracing` dependency. The `pub` fields on `CsrfConfig` (`header_name`, `expected_value`, `cookie_name`) are left public so struct-literal construction keeps compiling; a literal that clears `cookie_name` still goes through the `validate` warning path. README and the `identity-and-sessions` book chapter document the new names.
- **`AuthError` is no longer `Serialize`/`Deserialize`, and its `Display` no longer lists the caller's permissions (A2).** `ras-auth-core`: the derives are removed (nothing in the workspace serialized `AuthError`; generated servers already map it to a generic per-class message). `AuthError::InsufficientPermissions`'s `Display` now reads `Insufficient permissions: required [...], caller holds N permission(s)` — the `has` field is retained for server-side logging via `Debug`. **Breaking** for any downstream that serialized `AuthError` directly; map to a wire type of your own instead.
- **`DownloadResponse::attachment` escapes properly and emits an RFC 5987 `filename*` (F1).** `ras-file-core`: `"` and `\` are backslash-escaped in the quoted `filename="..."` form (previously `"` was stripped and `\` passed through), control characters are stripped, non-ASCII is replaced by `_` in the legacy form, and a `filename*=UTF-8''<percent-encoded>` parameter carries the original Unicode name. Tests that assert the exact `Content-Disposition` string need updating (in-repo: `ras-file-macro` e2e, `file-service-example`, `file-service-backend`).

### Added - 2026-09-05 (security hardening)
- **`ras_file_core::sanitize_filename(&str) -> String`** and **`ras_file_core::MAX_FILENAME_BYTES`** (F1). Reduces an untrusted filename to a single safe path component: keeps only the final component (split on both `/` and `\`), strips NUL and other control characters, maps dots-only names (`.`, `..`) and empty results to `"upload"`, and truncates to 255 bytes on a UTF-8 char boundary. Unicode is preserved.
- `ras-file-core` now depends on and re-exports `tracing` (`ras_file_core::tracing`) so generated `file_service!` code can log without consumers declaring a direct `tracing` dependency.

### Fixed - 2026-09-05 (security hardening)
- **Upload filenames are sanitized before they reach the handler (F1).** `file_service!`: the multipart `filename=` parameter is passed through `ras_file_core::sanitize_filename` before `IncomingFile::file_name()` sees it, so a handler that joins the name onto a directory cannot be steered by `../` or `..\` segments. `filename: required` / `forbidden` policies are still evaluated on the raw presence of the parameter.
- **axum rejection bodies are no longer echoed to the client (F2).** `file_service!`: `Multipart` extractor rejections, multipart parse errors, and `Path` extraction failures previously returned axum's own text (e.g. `Invalid boundary ...`, or the offending path value). They now return fixed messages — `invalid multipart request`, `invalid multipart body`, `invalid path parameters` — and log the axum detail at `warn`. `rest_service!`: `Path` and `axum_extra::Query` extractors previously used axum's default plain-text rejection, which echoes the offending value and target type (``Cannot parse `abc` to a `i32` ``). Generated handlers now take those extractors as `Result<_, Rejection>` and return `400` with the JSON body `{"error": "Invalid path parameters"}` / `{"error": "Invalid query parameters"}`, logging the detail at `warn` in line with the existing rejection-logging convention. Note: the `VersionMigration` error `Display` is still echoed on a `400` — that message is application-authored, like `RestError::message`, and unchanged.

### Changed - 2026-08-18 (`rest_service!` hardening — device-integration feedback)
- **`rest_service!` now requires `application/json` on bodied endpoints by default.** A request whose `Content-Type` is not `application/json` (parameters like `; charset=utf-8` are allowed) is rejected with `415 Unsupported Media Type` before the body is read. This forces a CORS preflight for cross-origin requests, closing the simple-request CSRF shape (a cross-origin `text/plain` POST), and matches `file_service!`, which already validated. **Breaking:** clients that POST/PUT/PATCH a body without an `application/json` content type now get `415`; opt out per-service with `require_json_content_type: false`. Rides in the already-unreleased `ras-rest-macro` `0.3.0`.

### Added - 2026-08-18 (`rest_service!` hardening)
- **`require_json_content_type: <bool>`** service option (default `true`) — opt out of the strict `Content-Type` check above.
- **`docs_require_auth: <bool>`** service option (default `false`) — gate the generated docs page and `openapi.json` behind authentication (any authenticated user) when `serve_docs` is enabled. Previously these routes were always public, exposing method names, schemas, and permission requirements; they remain public by default (conventional API-explorer behavior) and are now documented as such.
- **Per-endpoint `body_limit: <bytes>`** — override the service body limit for a single endpoint.
- **Per-endpoint `headers: true`** — pass the request `axum::http::HeaderMap` to the handler as an extra argument (after the caller/user, before path params), so header-derived data no longer requires a separate tower layer.

### Fixed - 2026-08-18 (`rest_service!` hardening)
- **`204 No Content` no longer carries a serialized body.** `RestResponse::no_content()` previously emitted a `204` with a `null` JSON body and `Content-Type: application/json`, violating RFC 9110; `204`/`304` responses now have an empty body.
- **`413` (too large) is distinguished from `400` (unreadable body).** A body whose declared `Content-Length` exceeds the limit is rejected up front; an over-limit streamed body is `413`, while a genuine stream read error is now `400` — previously both were reported as `413`.
- **Body-decode failures are logged.** A malformed JSON body is logged at `warn` with the serde error category and line/column (never the rejected value) before the generic `400`, matching the handler-error logging convention.
- **Authorization rejections are observable.** `401`/`403`/`500` responses from the shared authorize pipeline are logged at `warn` (server-side detail only); previously rejections bypassed both the usage and duration trackers and were logged nowhere.
- **A permissioned service built without an auth provider now panics at `build()`** with a clear message instead of returning a runtime `500` (`NoAuthProvider`) on the first request.

### Changed - 2026-08-18 (`jsonrpc_service!` parity)
- **`jsonrpc_service!` now requires `application/json` by default.** A request whose `Content-Type` is not `application/json` is rejected with `415` before the envelope is parsed. **Breaking** for clients that POST without that content type; opt out per-service with `require_json_content_type: false`. Rides in the already-unreleased `ras-jsonrpc-macro` `0.3.0`.
- `ras-jsonrpc-core` now depends on and re-exports `tracing` (`ras_jsonrpc_core::tracing`) so generated JSON-RPC server code can log without every consumer crate declaring a direct `tracing` dependency. Additive; folds into the already-unreleased `ras-jsonrpc-core` `0.2.0`.

### Added - 2026-08-18 (`jsonrpc_service!` parity)
- **`require_json_content_type: <bool>`** service option (default `true`) — opt out of the strict `Content-Type` check.
- **`body_limit: <bytes>`** service option (default 2 MiB) — cap the request body size via a `DefaultBodyLimit` layer.
- **`docs_require_auth: <bool>`** service option (default `false`) — gate the explorer page and `openrpc.json` behind authentication (any authenticated user) when the explorer is enabled. The RPC endpoint itself is never gated by this option. Previously these routes were always public.

### Fixed - 2026-08-18 (`jsonrpc_service!` parity)
- **Malformed request bodies are logged.** A JSON parse failure is logged at `warn` with the serde error category and line/column (never the rejected value) before the `-32700` parse error.
- **Authorization rejections are observable.** `401`/`403` responses (authentication required, token expired, CSRF, insufficient permissions) are logged at `warn`; previously rejections were logged nowhere and bypassed the usage tracker.
- **`build()` now fails when a permissioned service (or a gated explorer) has no auth provider**, returning a clear `Err(String)` instead of silently rejecting every such call at runtime.
- **Dependency advisory:** bumped `h2` `0.4.13` → `0.4.16` for RUSTSEC-2026-0258 (unbounded empty DATA frames); `cargo deny check advisories` and `cargo audit` are clean again (transitive via the axum/hyper/reqwest HTTP stack).

### Changed - 2026-08-18 (multi-agent review remediation)
- **Semver:** bumped six crates that re-export a bumped dependency in their public API — `ras-rest-core` `0.1.1` → `0.2.0`, `ras-file-core` `0.1.0` → `0.2.0`, `ras-observability-core` / `ras-observability-otel` `0.1.0` → `0.2.0`, `ras-jsonrpc-bidirectional-types` / `ras-jsonrpc-bidirectional-client` `0.1.0` → `0.2.0` — and cascaded the `{ path, version }` requirements. (Also records the earlier `ras-jsonrpc-bidirectional-macro` `0.1.0` → `0.2.0` bump for the M4 compile-error contract.)
- **REST generated code logs via `ras_rest_core::tracing`.** `ras-rest-core` now depends on and re-exports `tracing`, mirroring the JSON-RPC fix, so `rest_service!` consumers no longer need an undeclared direct `tracing` dependency.
- Corrected every documented dependency version pin (book pages + crate READMEs) to the current crate versions; stale pins would have resolved a pre-hardening macro or linked two incompatible copies of a core crate.

### Fixed - 2026-08-18 (multi-agent review remediation)
- **Generated REST client tolerates an empty 204/304 success body.** A `204` on a non-unit response type now deserializes as `null` (so `Option<T>` / `serde_json::Value` resolve to `None` / `Null`) instead of failing with a serde EOF error; the server also omits the body for `205 Reset Content`.
- **OAuth2 reserved-parameter denylist widened (H1).** `request`, `request_uri`, `response_mode`, `resource`, `audience`, and `id_token_hint` are now rejected in `additional_params` / provider `auth_params`, closing the OIDC request-object override path.
- **OAuth2 id_token `sub` is now required (M6).** `validate_id_token_claims` rejects an id_token without a `sub` claim, and the userinfo↔id_token subject binding fails closed instead of silently no-opping.
- **CSRF header names are validated (L2 follow-up).** `CsrfConfig::validate()` rejects a CORS-safelisted or browser-controlled header name (`accept`, `content-type`, `cookie`, …), which would otherwise satisfy the fail-closed cookie/CSRF check while providing no protection.
- Documentation fidelity: `ras-jsonrpc-types` README uses the single-argument `insufficient_permissions`; `ras-auth-core` README states the cookie-always-carries-CSRF invariant; the `docs_require_auth` browser-transport limitation, the Content-Type gate's bodied-endpoint scope, and the `headers: true` credential-exposure caveat are now documented.

### Changed - 2026-08-12 (security review remediation)
- **Cookie auth now requires CSRF (H2).** `AuthTransportConfig::validate` rejects a config with a cookie transport and no CSRF config, and `with_cookie(...)` / the generated `auth_cookie(...)` builders now install a default double-submit `CsrfConfig` when none is set. There is no builder path to cookie auth without CSRF. Existing apps that enabled cookies and omitted CSRF will now fail at `build()`/`validate()` — this is intended. Bumped `ras-auth-core` `0.1.0` → `0.2.0`, `ras-rest-macro` `0.2.1` → `0.3.0`, `ras-jsonrpc-macro` `0.2.0` → `0.3.0`, `ras-file-macro` `0.1.0` → `0.2.0`.
- **Empty permission group mixed with non-empty groups no longer grants any authenticated user (M4).** `WITH_PERMISSIONS(["admin"] | [])` previously granted access to any logged-in user; it now denies at runtime (`check_permission_groups` / `user_satisfies_permission_groups` in `ras-auth-core`) and is a compile error in all four service macros. `WITH_PERMISSIONS([])` (authenticated-only) is unchanged. Generated-code contract change on the REST/JSON-RPC/file/bidirectional macros.
- **JSON-RPC `-32002` no longer returns the caller's permission set (M1).** `JsonRpcError::insufficient_permissions` now takes only `required` and omits `has` from the error `data`; the caller's grant set is never echoed. Public JSON shape and function-signature change. Bumped `ras-jsonrpc-types` `0.1.1` → `0.2.0`, `ras-jsonrpc-core` `0.1.2` → `0.2.0`.
- **WebSocket JSON-RPC and upgrade errors are sanitized (H3).** Handler and `AuthError` internals are no longer stringified onto the wire; `ServerError::client_message()` returns a generic per-class message (the full error is logged server-side). Matches the HTTP JSON-RPC / REST sanitization. Bumped `ras-jsonrpc-bidirectional-server` `0.1.0` → `0.2.0`.
- **WebSocket credential extraction matches HTTP (M5).** Only `Authorization: Bearer <token>` (case-insensitive, non-empty) is treated as a bearer token; raw values and non-Bearer schemes are rejected and a malformed header no longer falls through to a weaker transport. Client-claimed IP metadata is relabelled `claimed_client_ip` (untrusted).
- **OAuth2 default start-flow binds against login CSRF (M2).** `OAuth2Provider::start_flow` now generates a session binding and returns it in `OAuth2Response::AuthorizationUrl { url, state, binding }` (new field); the callback must echo it. `start_flow_bound(.., None)` remains the explicit unbound escape hatch. Bumped `ras-identity-oauth2` `0.1.2` → `0.2.0`.
- **OAuth2 reserved-parameter injection blocked (H1).** `additional_params` / provider `auth_params` can no longer override reserved OAuth/OIDC parameters (`redirect_uri`, `state`, PKCE, etc.); a collision returns the new `OAuth2Error::InvalidAuthorizationParam`.
- **OAuth2 ID-token / client hardening (M6).** `issuer` is now required (fail-closed) to accept an id_token; userinfo `sub` must match the id_token `sub`; multi-audience tokens require a matching `azp`; `OAuth2Client::new` no longer silently falls back to a timeout-less HTTP client (it now panics — use `try_new`).
- **JWT sessions gain optional `iss`/`aud` (M3).** `SessionConfig` and `JwtClaims` carry optional `iss`/`aud`; when configured they are encoded and verified, rejecting cross-service token reuse. Removed the unused `refresh_enabled` flag (no refresh-token rotation exists). Bumped `ras-identity-session` `0.2.0` → `0.3.0`.

### Fixed - 2026-08-12 (security review remediation)
- **Secrets no longer appear in `Debug` (L1).** `SessionConfig` (`jwt_secret`), `OAuth2ProviderConfig` (`client_secret`), and `LocalUser` (`password_hash`) now use redacting `Debug` impls. OAuth2 token-exchange / userinfo error paths log the status code only, not the response body. Bumped `ras-identity-local` `0.2.0` → `0.2.1`.
- **CSRF token comparison is constant-time (L2).** `CsrfConfig::validate_headers` uses `subtle::ConstantTimeEq` (new direct dependency of `ras-auth-core`).
- **Dependency advisories (D1).** `cargo update` bumped `crossbeam-epoch` (`0.9.18` → `0.9.20`), `quinn-proto` (`0.11.14` → `0.11.16`), and `spin` (`0.9.8` → `0.9.9`); `cargo deny check advisories` and `cargo audit` are clean. `lru`/`paste` warnings remain (transitive via `ratatui` in the TUI example only; not compiled into any library crate).
- **oauth2-demo hardened (H4).** Dropped the client-controlled `additional_params`; the JWT is delivered in the URL fragment (never sent to the server / not in `Referer`) and scrubbed from the URL immediately instead of the query string; a login-CSRF binding cookie is set and verified; `enforce_active_sessions` is enabled; CORS is restricted to the demo origin; and admin permissions are only granted on a verified email.

### Documentation - 2026-08-12
- `identity-and-sessions.md` and crate READMEs describe cookie auth as cookie **and** CSRF (H2) and the bound OAuth2 flow as the primary path (M2).
- The root README's "Rate Limiting" bullet is corrected: the local-auth `Semaphore` is a concurrency bound, not a rate limiter (L3).

### Added - 2026-06-29
- New `OPTIONAL_AUTH` route level for `rest_service!`, `file_service!`, `jsonrpc_service!`, and `jsonrpc_bidirectional_service!`. An `OPTIONAL_AUTH` route is public — never rejected for auth reasons — but opportunistically identifies its caller: the handler receives a `ras_auth_core::Caller` (`Anonymous` / `Authenticated(user)`) as its first argument (the file service surfaces it through `FileRequestContext`). Resolution is fully lenient: a missing, invalid, or expired credential, or a cookie that fails CSRF on an unsafe method, resolves to `Caller::Anonymous` rather than a 401/403.
- `ras-auth-core`: new `Caller` enum (`#[must_use]`) and non-rejecting `resolve_caller` resolver alongside `authorize_request`.
- `ras-permission-manifest`: new `AuthRequirementInfo::Optional` variant so manifests distinguish `OPTIONAL_AUTH` from public/authenticated operations; `SCHEMA_VERSION` bumped to `2` (older pinned consumers will fail to deserialize a manifest containing `"type":"optional"`).
- OpenAPI emits an optional security requirement (`[{}, {"bearerAuth": []}]`) and OpenRPC emits `x-authentication: { required: false }` for `OPTIONAL_AUTH` operations.
- Existing `UNAUTHORIZED` and `WITH_PERMISSIONS` behavior is unchanged.

### Changed - 2026-06-06
- REST, JSON-RPC, and file generated-client APIs are now consistent: builders take the URL at construction, auth state is cloned, `build_with_transport(...)` is always available for generated clients, public timeout variants take `Duration`, and default reqwest-backed `build()` is emitted only when the macro crate's `reqwest` feature is enabled.
- Macro client features now distinguish transport-injected clients from default reqwest clients: `client` emits generated clients using `ras-transport-core`, while `reqwest` enables the default `ReqwestTransport` constructor.
- Documentation now describes the `client`/`reqwest` split, direct `ras-transport-core` dependency requirements for generated client consumers, and native file-client `fs` helpers.

### Changed - 2026-05-24
- Specification types crate now uses the `ras-openrpc-types` package name and `ras_openrpc_types` import path.
- Package metadata, clone instructions, and documentation links now point to the moved `rust-api-stack` repository.

### Fixed - 2026-05-23
- `ras-identity-local`: Duplicate local user creation now fails with `LocalUserError::UserAlreadyExists` instead of silently overwriting credentials.
- Bumped `ras-identity-local` from `0.1.1` to `0.2.0` because `LocalUserProvider::add_user` now returns the crate-specific `LocalUserError`.
- Bumped `ras-identity-oauth2` from `0.1.1` to `0.1.2` for the additive `UserInfoMapping` root re-export and updated OAuth2 docs.
- Bumped `ras-identity-session` from `0.1.1` to `0.2.0` because replacing `jsonwebtoken` exposes the crate-local `JwtAlgorithm` and string-backed JWT errors in the public API.
- `documentation/ras-identity.md`: Identity examples now use the current `UserPermissions`, `SessionService`, JWT claims, session revocation, and Axum 0.8 server APIs.
- `ras-identity-local`: README testing/security notes now distinguish default tests from optional timing-sensitive checks.
- `ras-identity-session`: JWT signing now uses local HMAC-SHA implementations for HS256/HS384/HS512 instead of pulling in the broader `jsonwebtoken` RustCrypto/RSA dependency path.
- `ras-openrpc-types`: Restored the original `Extensions::insert`, `Extensions::with`, and `Extensions::from_map` signatures for compatibility; checked variants are now available as `try_insert`, `try_with`, and `try_from_map`.
- `ras-jsonrpc-macro`: Version labels such as `"1.0.0"` and `"v1-beta"` now generate sanitized client method suffixes instead of invalid Rust identifiers.
- Supply-chain policy now passes on current `cargo-deny`; vulnerable `rand`, `time`, `tracing-subscriber`, `protobuf`, and related OpenTelemetry/Prometheus dependencies were updated, and unmaintained `wee_alloc` was removed from the WASM UI example.
- `examples/bidirectional-chat`: Auth lifecycle tests now verify login after registration, duplicate registration rejection, and permission-bearing JWT claims.
- `examples/bidirectional-chat`: Removed fake auth endpoint checks from `server_tests.rs`; auth endpoint coverage now lives in the in-memory lifecycle suite that wires the real identity/session stack.
- `examples/bidirectional-chat`: Configuration docs now match the implemented config-file and environment-variable loading behavior.
- `examples/bidirectional-chat`: README commands now use the actual `bidirectional-chat-tui` package and current example credentials.
- `examples/bidirectional-chat`: TUI README now states the correct Rust 1.88+ requirement for Rust 2024 edition crates.
- `examples/file-service-wasm`: README now names the real `wasm-client` feature.
- `ras-openrpc-types` and `ras-jsonrpc-types`: README dependency snippets now match the current crate versions.
- REST and JSON-RPC macro documentation dependency snippets now match the workspace Axum, Tokio, and schemars versions.
- `ras-rest-macro` and `ras-jsonrpc-macro`: HTTP integration tests now use in-memory `axum-test` mock transport instead of binding local TCP sockets.
- `ras-jsonrpc-macro`: Generated-client compile/config coverage no longer attempts requests against an unused localhost port.
- CI now treats clippy warnings as failures with `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Removed unused workspace dependency declarations left behind by older local tooling and UI experiments.
- Narrowed JSON ignore rules so TypeScript example `tsconfig` files are visible for version control while generated OpenRPC/OpenAPI and runtime data stay ignored.
- `examples/file-service-wasm` and `examples/rest-wasm-example`: TypeScript generated-client samples are now plain usage examples instead of standalone npm apps.
- CI now verifies the generated OpenAPI specs used by the TypeScript usage samples without installing npm dependencies for those samples.
- `examples/wasm-ui-demo`: Added a local README and fixed the browser client/proxy path to match the basic JSON-RPC service's `/rpc` route.
- `examples/wasm-ui-demo`: Build scripts and ignore rules now match the actual Rollup `dist/` output.
- CI now builds the `wasm-ui-demo` WebAssembly bundle with the `wasm32-unknown-unknown` target.
- CI now enforces the tracked `deny.toml` supply-chain policy with `cargo-deny`.
- `examples/file-service-example`: Added a local README with run instructions, curl examples, and token behavior.
- Root and bidirectional JSON-RPC README examples now match the current generated client/server APIs and avoid overstating retry behavior.
- `ras-jsonrpc-bidirectional-client`: The documented WASM feature check now compiles with `wasm32-unknown-unknown` and keeps native WebSocket dependencies out of the WASM dependency graph.
- `ras-identity-oauth2`: OAuth2 integration tests now use in-memory `axum-test` mock transport instead of socket-bound mock HTTP servers.
- `ras-jsonrpc-bidirectional-server` and `ras-jsonrpc-bidirectional-macro`: WebSocket handler, generated-service, and round-trip benchmark coverage now run through an in-memory socket adapter instead of binding local TCP ports.
- `ras-jsonrpc-bidirectional-server`: Request handler failures now return JSON-RPC error responses and keep the WebSocket loop alive for later requests.
- `ras-jsonrpc-bidirectional-client`: Native transport request construction and disconnected send/receive behavior now have socketless unit coverage.
- `ras-identity-oauth2`: Added fake-transport client tests for state mismatch, provider callback errors, PKCE-disabled token exchange parameters, and missing userinfo endpoint handling.
- `examples/bidirectional-chat`: Added runtime `messages_per_minute` enforcement for authenticated `send_message` calls, plus socketless WebSocket flow tests for room join, list, leave, profile update/readback, moderator kick, admin announcement broadcast, generated permission denial, request-error recovery, disconnect cleanup, typing cleanup, message rate limiting, and multi-user message broadcast through the generated handler, in-memory adapter, and in-memory connection manager; profile avatar persistence now uses the same snake_case strings as the API.
- `examples/file-service-wasm`: Corrected the documented 100 MB upload limit and generated OpenAPI path in the TypeScript usage sample.
- Root README quick start now keeps the first-run path Rust-only and points to frontend examples as optional follow-ups.
- REST macro docs now describe the built-in API explorer and point to the actual `/docs/openapi.json` route.
- TypeScript client docs now describe OpenAPI-generated fetch-client usage without implying a framework or npm app scaffold.
- Changelog history no longer implies the current `.cargo/config.toml` configures Kellnr as the default registry.
- `examples/bidirectional-chat`: Server test README now describes remaining WebSocket coverage as in-memory handler testing.
- `examples/wasm-ui-demo`: Removed an unused placeholder resources directory.
- `ras-jsonrpc-bidirectional-macro`: README feature docs now match the actual `server`/`client` feature set, and documented `server_to_client_calls` syntax is covered by parser and compile tests.
- `ras-jsonrpc-bidirectional-macro`: Generated server-to-client RPC handlers now wrap callbacks in `Arc` instead of requiring an undocumented `Clone` bound.
- `ras-jsonrpc-bidirectional-server`: Manager tests no longer reference the deleted socket-bound integration test file.
- Root, REST macro, and observability docs no longer contain placeholder implementation comments or undefined sample variables in their primary setup snippets.
- Package README test commands now consistently use `--locked`, and the OAuth2 demo's focused test example names a real current test.
- OAuth2 README and demo landing-page copy now use current project naming and avoid implying unimplemented response caching or active-session token revocation.
- Example run/check/build snippets now consistently use the checked-in lockfile.
- Root, example overview, and local example quick-start commands now use workspace-root package invocations where practical instead of mixing `cd`-based forms.
- `examples/bidirectional-chat`: Workspace-root server commands now set `CHAT_DATA_DIR` alongside `CHAT_CONFIG_FILE` so persisted chat state lands under the ignored example runtime directory.
- Root, examples, Playwright, and CI metadata now state the Rust 1.88+ and Node.js 22.13+ prerequisites consistently.
- Cargo package manifests now declare `rust-version = "1.88"` to match the locked workspace dependency graph.
- File-service macro installation docs now list the native and WASM dependencies required by the generated server and clients.
- REST and JSON-RPC macro installation snippets now wire consumer crate `server` and `client` features to the macro features and optional dependencies.
- Bidirectional client docs now describe caller-managed reconnect behavior instead of claiming an automatic reconnect loop, and example snippets use concrete demo tokens and real package commands.
- File-service macro docs and example READMEs now use current generated trait names, concrete upload/download snippets, and checked-in backend links instead of placeholder storage/auth code.
- REST macro docs now use the current `AuthProvider`/`AuthFuture` shape, concrete demo auth providers, valid OpenAPI configuration examples, and complete generated trait method lists instead of placeholder code.
- JSON-RPC macro and core docs now use concrete method definitions, generated builder declarations, and current `AuthProvider` permission-checking examples instead of placeholder helper APIs.
- `ras-observability-core`: Added `RequestContext::websocket(method)` and updated observability/identity examples to use concrete env-backed configuration instead of placeholder credentials and pseudo-code.
- Bidirectional JSON-RPC and OpenRPC type docs now use concrete validation/sender examples, and `ras-jsonrpc-bidirectional-types` re-exports `MessageSenderExt` from the crate root to match the documented API.
- Identity, observability, bidirectional WebSocket, and JSON-RPC types docs now avoid broad "everything"/"complete"/"high-performance" claims unless the text is tied to a concrete implemented API.
- REST macro TypeScript snippets now avoid ambiguous ellipsis-style config spreading in favor of explicit request option construction.
- `examples/rest-wasm-example/rest-backend`: Added a backend-local README with run commands, demo tokens, generated OpenAPI locations, endpoint map, and focused test commands.
- `examples/rest-wasm-example/rest-api`: Added a shared-contract README covering generated server/client features and related example files.
- `examples/bidirectional-chat/server`: Added a server-local README with run commands, configuration behavior, REST auth endpoints, WebSocket auth options, and socketless test guidance.
- Example API crates now have package-local READMEs that describe their generated contracts, feature flags, related runnable examples, and focused check commands.
- Playwright fixture crates now have local READMEs that document their browser-test role, socket-bound ports, routes, test tokens, and focused check commands.
- `examples/wasm-ui-demo`: Trimmed direct npm build dependencies by using Node's built-in directory removal and removing the extra terser Rollup plugin from the example build.
- `examples/wasm-ui-demo` and `ras-rest-macro`: Removed stale direct Cargo dependency declarations that are no longer used by the example UI or REST macro tests.
- REST macro installation docs now list the consumer-side `axum-extra` dependency required by generated query-parameter extractors.
- Public guides now avoid broad "complete" claims for examples and use concrete labels such as runnable service, task API example, and file API example.
- Example READMEs now use correct relative paths for backticked local file references that are not covered by Markdown link checking.
- `basic-jsonrpc-api` and `rest-api`: Added direct contract tests for generated OpenRPC/OpenAPI documents and important wire shapes used by generated clients.
- `oauth2-demo-api` and `bidirectional-chat-api`: Added direct contract tests for generated OpenRPC permissions, schema metadata, and bidirectional notification/avatar wire shapes.
- Playwright fixture crates now have socketless contract tests for generated OpenRPC/OpenAPI methods, routes, docs, auth metadata, query parameters, and version metadata.
- `ras-jsonrpc-core`: Added re-export contract tests for auth types, JSON-RPC protocol types, and version migration traits.
- CI now checks Cargo package README targets and local Markdown links without adding repository scripts.
- Root README now documents the documentation-hygiene checks that CI runs for package README targets and local Markdown links.
- Security and observability docs now describe concrete mitigations and setup hooks instead of broad constant-time or zero-configuration claims.

### Removed - 2026-05-23
- Removed stale local development artifacts: the `ras-file-macro` debug proc-macro stub, the bidirectional chat `test-config` diagnostic binary, and a tracked runtime chat log.
- Removed tracked local-agent and scratch artifacts from `.claude/`, `agent-research/`, `docs_and_help/`, and `sketchpad/`; these paths are now ignored for local use only.
- Removed socket-bound HTTP mock dev-dependencies left behind by older OAuth2 and macro test suites.
- Removed unused `tokio-test` dev-dependencies and the stale bidirectional chat server `reqwest` dev-dependency left behind after the socketless test cleanup.
- Removed scaffold-style placeholder comments from `deny.toml` so the tracked supply-chain policy is project-specific.
- Added the current `Unicode-3.0` SPDX license identifier to `deny.toml`.

### Added - 2026-05-10
- Added `ras-version-core` `0.1.0` with the shared `VersionMigration<From, To>` trait for opt-in API compatibility migrations.
- `ras-jsonrpc-macro`: Added opt-in versioned JSON-RPC methods. Legacy wire methods can migrate legacy requests into canonical request types, call the canonical trait method, and migrate canonical responses back to legacy response types.
- `ras-rest-macro`: Added opt-in versioned REST endpoints. Legacy routes can migrate generated request-part structs into canonical request parts before invoking the canonical service method, then migrate response bodies back to legacy response types.
- `ras-jsonrpc-macro` and `ras-rest-macro`: Generated clients and OpenRPC/OpenAPI specs now include versioned compatibility methods/routes when configured.
- Added REST and JSON-RPC Playwright explorer coverage for versioned compatibility routes and wire methods.

### Fixed - 2026-05-10
- `ras-rest-macro`: Generated REST clients now serialize query parameters through reqwest's serde-backed query path, support repeated-key `Vec<T>` and `Option<Vec<T>>` query params, and honor serde-renamed enum values without requiring `Display`. Fixes #3.

### Changed - 2026-05-10
- `ras-jsonrpc-macro`: Generated service setup now matches REST's trait-backed model. Users implement the generated service trait and pass the implementation to `ServiceBuilder::new(service)`, with `.base_url(...)` for custom JSON-RPC route paths.
- Bumped `ras-jsonrpc-macro` from `0.1.2` to `0.2.0` because the generated JSON-RPC server setup changed from handler setters to a required service trait implementation.
- Bumped `ras-jsonrpc-core` from `0.1.1` to `0.1.2` for the additive `VersionMigration` re-export.
- Bumped `ras-rest-core` from `0.1.0` to `0.1.1` for the additive `VersionMigration` re-export.
- Bumped `ras-rest-macro` from `0.2.0` to `0.2.1` for additive versioned endpoint/client/spec generation.
- Bumped `ras-rest-macro` from `0.1.1` to `0.2.0` because generated client query params now use serde serialization instead of `Display`/`ToString`.

### Documentation - 2026-05-10
- Updated JSON-RPC, REST, identity, observability, example, and Playwright documentation for trait-backed service setup, current auth syntax, current crate names, and versioned API migration examples.

### Removed - 2026-05-22
- Removed the `openrpc-to-bruno` tool crate from the workspace.

### Added - 2026-05-09
- Established repository versioning and changelog policy in `VERSIONING.md`.
- Added doc-comment support for generated API documentation:
  - `ras-jsonrpc-macro` now maps `///` comments on JSON-RPC methods into OpenRPC `summary` and `description`.
  - `ras-rest-macro` now maps `///` comments on REST endpoints into OpenAPI operation `summary` and `description`.
- Enhanced the API explorer to render documentation from generated specs:
  - Shows operation/method docs for both REST and JSON-RPC.
  - Shows schema/type and field descriptions produced by `schemars::JsonSchema`.
  - Renders a safe dependency-free Markdown subset for paragraphs, line breaks, bold, inline code, fenced code blocks, lists, and HTTP(S) links.
- Added Playwright e2e coverage for REST and JSON-RPC explorer documentation rendering.

### Changed - 2026-05-09
- Bumped `ras-jsonrpc-macro` from `0.1.1` to `0.1.2`.
- Bumped `ras-rest-macro` from `0.1.0` to `0.1.1`.

### Added - 2025-01-14
- Cat avatar system for bidirectional chat users
  - Unique ASCII art cat avatars generated from username hashes
  - Multiple cat breeds, colors, and expressions
  - Animated states (normal, blinking, winking, happy)
  - Typing indicators with animated speech bubbles
- Enhanced chat UI with chat bubbles and timestamps
- Message persistence system using JSON files
  - State files for rooms and user profiles
  - JSONL message logs per room
  - Automatic state recovery on server restart
- User sidebar showing active users in rooms
- Real-time typing indicators in both server and TUI client

### Refactored - 2025-01-14
- Migrated bidirectional chat server authentication endpoints to use REST macro
  - Replaced manual Axum handlers with type-safe REST service definitions
  - Added structured request/response types with JSON Schema support
  - Improved error handling with proper HTTP status codes
  - Enabled OpenAPI documentation generation for auth endpoints

### Changed - 2025-01-14
- Removed unused MCP server configurations (language-server, human-in-the-loop) from .mcp.json
- Updated .gitignore to exclude local chat server config.toml and test scripts

### Fixed - 2025-01-14
- Updated minimum password length in chat server config examples to match 8-character validation requirement

### Refactored - 2025-01-14
- Simplified identity provider setup in bidirectional chat server
  - Removed unnecessary Arc wrapper for initial identity provider
  - Created separate registration provider instance sharing same user data
  - Improved code clarity while maintaining same functionality

### Added - 2025-01-14
- Bidirectional chat terminal client foundation (Sprint 2 Day 1)
  - Modular architecture with separate ui, client, auth, and config modules
  - Complete ratatui-based terminal UI with message area, user list, and input field
  - Initial WebSocket client integration scaffolding
  - Configuration system supporting environment variables and TOML files
  - JWT token management infrastructure for authentication

### Updated - 2025-01-14
- Simplified local development guidance to use generic examples instead of listing all crates
- Added bidirectional chat client architecture details to documentation
  - Terminal UI layout and components
  - State management and WebSocket integration
  - Authentication and configuration details
- Documented successful completion of the bidirectional chat server and client foundation

### Added - 2025-01-13
- Comprehensive configuration system for bidirectional chat server
  - Flexible configuration supporting environment variables and TOML files
  - Server, auth, chat, logging, admin, and rate limit settings
  - Legacy environment variable support for backward compatibility
  - Configuration validation with helpful error messages
  - Example config file and test utility for validation

- Structured logging with tracing for bidirectional chat server
  - Configurable log levels and formats (pretty, JSON, compact)
  - Structured logging with connection IDs, user info, and room details
  - Debug/trace logging for detailed troubleshooting
  - Configuration via RUST_LOG environment variable or config file

- Comprehensive integration tests for bidirectional chat server
  - Server integration tests covering startup, config, auth, and persistence
  - WebSocket tests for connection lifecycle and authentication
  - Concurrent user scenarios and permission handling tests
  - Port management for parallel test execution
  - Complete test coverage of all server features

- Enhanced persistence layer with structured logging
  - Added tracing to all file operations and state management
  - Error context with detailed failure messages
  - Parse error tracking when loading corrupted messages
  - Operation metrics for state loading/saving

### Added - 2025-01-13
- Bidirectional chat example demonstrating real-time WebSocket communication
  - Complete chat server with room management and message persistence
  - CLI client with register/login/chat commands for interactive sessions
  - JWT-based authentication with role-based permissions (user/admin)
  - Persistent chat history using JSON file storage
  - Type-safe bidirectional RPC using generated client/server code
  - Added bidirectional macro implementation notes

- User profile system with cat avatar customization
  - Added profile management endpoints (get_profile, update_profile)
  - Support for 10 cat breeds, 10 colors, and 8 expressions
  - Integrated profile persistence with existing state management
  - Profile creation during user registration

### Fixed - 2025-01-09
- Fixed bidirectional WebSocket channel management synchronization issue causing test failures
  - Extended ConnectionManager trait with add_connection_with_sender method for proper channel registration
  - Fixed WebSocket service to register actual message channels instead of creating dummy channels
  - Resolved "channel closed" errors and timeout issues in bidirectional communication tests
  - Enhanced DefaultConnectionManager to handle real channel registration via downcasting
  - All 22 bidirectional JSON-RPC tests now pass with proper connection management

### Added - 2025-01-09
- Enhanced bidirectional JSON-RPC macro with server-side client management capabilities
  - Service trait methods now receive client connection ID and connection manager reference 
  - Connection lifecycle hooks: on_client_connected, on_client_disconnected, on_client_authenticated
  - Typed client handles for direct server-to-client communication and connection management
  - Real-time broadcasting capabilities within service implementations
  - Full access to connection manager for advanced client tracking and messaging patterns

### Added - 2025-01-09
- Type-safe client generation for both JSON-RPC and REST services with comprehensive API coverage
  - Implemented builder pattern client APIs with reqwest for HTTP communication
  - Added feature flags (server/client) for optional dependency management and modular builds
  - Bearer token authentication support with get/set methods for secure API access
  - Timeout configuration for both default and per-request timeout handling
  - Cross-platform compatibility using reqwest for both x86 and WASM targets
  - Generated client methods match server API signatures exactly for type safety
  - Zero breaking changes with full backward compatibility for existing server-only code
  - Optional client dependencies (reqwest) only loaded when client feature enabled
  - Comprehensive test coverage for client generation and HTTP communication patterns

### Fixed - 2025-01-09
- Fixed OpenRPC schema generation to comply with JSON-RPC specification
  - Schema definitions now properly use components/schemas instead of $defs
  - Service-specific helper functions prevent naming conflicts in generated code
  - All schema references updated to use standard #/components/schemas/ format

### Refactored - 2025-01-09
- Restructured Google OAuth example into multi-crate architecture for better separation of concerns
  - Split into separate `api` and `server` crates with clean API boundary separation
  - API crate contains service definitions and OpenRPC generation logic
  - Server crate focuses on HTTP routing, authentication, and frontend serving
  - Build-time OpenRPC generation moved to build.rs for automatic documentation updates
  - Improved static file serving with relative paths for better deployment flexibility
  - Enhanced example structure provides clearer patterns for real-world applications

### Enhanced - 2025-01-09
- Updated workspace configuration and dependencies to support new tooling and improved development experience
  - Updated schemars to 1.0.0-alpha.20 for improved JSON Schema Draft 7 compatibility
  - Enhanced workspace member organization for multi-crate example structure
  - Fixed import ordering in integration tests following Rust style guidelines
  - Improved Cargo.lock with new dependencies for CLI tools and testing infrastructure

### Fixed - 2025-01-09
- Fixed OpenRPC specification parsing to support extension fields and JSON Schema compatibility
  - Removed deny_unknown_fields restrictions from Method and Schema structs in ras-openrpc-types crate
  - Added $schema field support to Schema struct for proper JSON Schema Draft 7 compatibility
  - Enables proper parsing of OpenRPC documents with x-authentication and x-permissions extensions

### Enhanced - 2025-01-09
- Enhanced OpenRPC document generation functionality to actually generate files
  - Modified the OAuth2 demo to call OpenRPC generation functions during service creation
  - Added JsonSchema derives to all request/response types for proper schema generation
  - Created test infrastructure to verify end-to-end OpenRPC generation works correctly
  - OpenRPC documents now properly written to target/openrpc/ directory when enabled

### Documentation - 2025-01-09
- Added comprehensive OpenRPC generation documentation to ras-jsonrpc-macro README
  - Documented OpenRPC generation feature with complete usage examples and configuration options
  - Included requirements for JsonSchema trait implementation on request/response types
  - Added examples for both boolean and custom path OpenRPC generation configurations
  - Explained generated function signatures and integration patterns

### Enhanced - 2025-01-08
- Refactored permission system to support AND/OR logic groups for both REST and JSON-RPC macros
  - Changed permission syntax from flat array to nested groups with OR logic between groups and AND logic within groups
  - `WITH_PERMISSIONS(["admin", "moderator"])` now requires user to have both admin AND moderator permissions
  - `WITH_PERMISSIONS(["admin", "moderator"] | ["super_user"])` allows (admin AND moderator) OR super_user access
  - Supports multiple OR groups for complex permission combinations
  - Updated both REST and JSON-RPC macros simultaneously to ensure consistent behavior
  - Enhanced test coverage with new test cases demonstrating OR group functionality
  - Backward compatible syntax for existing single-group permissions
  - OpenAPI and OpenRPC documentation generation handles new permission structure correctly

### Fixed - 2025-01-08
- Fixed REST macro integration test failures with improved error handling and permission logic
  - Enhanced JSON error handling to return proper 400 status codes instead of 422 for invalid JSON requests
  - Fixed permission checking logic to use OR semantics (user needs ANY of the required permissions) instead of AND semantics
  - Improved macro-generated code to handle JSON parsing errors gracefully with appropriate HTTP status codes
  - Resolved test failures in `test_multiple_permissions_endpoints` and `test_invalid_requests`
  - Permission system now properly allows users with any of the listed permissions to access endpoints

### Fixed - 2025-01-08
- Fixed REST service example endpoint syntax for empty parameter methods
  - Corrected auth/logout and auth/me endpoint definitions to use proper empty parameter syntax
  - Updated handler signatures to match macro-generated function signatures for parameterless endpoints
  - Improved consistency with REST macro patterns for endpoints that don't require request bodies

### Fixed - 2025-01-08
- Fixed JSON-RPC macro parameter handling for unit type `()` parameters
  - Enhanced macro-generated code to properly handle methods with unit type parameters when no params are provided
  - Fixed parameter parsing to deserialize `None` parameters as `serde_json::Value::Null` for unit types instead of rejecting as invalid
  - Resolved test failures in `test_unauthorized_methods`, `test_authentication_required_methods`, `test_admin_permission_methods`, and `test_concurrent_requests`
  - Improved backward compatibility for JSON-RPC requests with missing or null parameters for void methods

### Added - 2025-01-08
- Comprehensive HTTP integration test suites for both JSON-RPC and REST macro crates
  - Complete JSON-RPC integration tests covering all authentication patterns (UNAUTHORIZED, WITH_PERMISSIONS with various levels)
  - Full REST API integration tests with CRUD operations, path parameters, and HTTP method validation
  - HTTP integration coverage for generated routers and clients
  - Authentication and authorization testing across all permission levels with JWT token validation
  - Security testing including timing attack resistance and proper error handling scenarios
  - Concurrent request testing validating thread safety and performance under load
  - OpenRPC and OpenAPI document generation testing ensuring specification compliance
  - Test infrastructure supporting both positive and negative scenarios with comprehensive error validation
  - Fixed unused import warnings in `ras-identity-local` during test infrastructure development

### Enhanced - 2025-01-08
- Added comprehensive testing dependencies for HTTP integration testing across macro crates
  - Added HTTP client, router, concurrency, and async helper dependencies for robust HTTP testing infrastructure
  - Enhanced `ras-jsonrpc-macro` and `ras-rest-macro` with testing dependencies for real server integration tests
  - Established foundation for comprehensive integration testing and concurrent request handling
  - Dependencies support both JSON-RPC and REST API testing patterns with authentication validation

### Refactored - 2025-01-08
- Architectural refactoring to eliminate coupling between RPC and REST macro crates
  - Created new `ras-auth-core` crate as shared foundation for authentication types and traits
  - Moved `AuthProvider`, `AuthenticatedUser`, `AuthError`, and related types from `ras-jsonrpc-core` to `ras-auth-core`
  - Updated `ras-rest-macro` to depend on `ras-auth-core` instead of `ras-jsonrpc-core`, eliminating unwanted cross-dependencies
  - Updated `ras-identity-session` and other affected crates to use shared authentication types
  - Maintained full backward compatibility through re-exports in `ras-jsonrpc-core`
  - Enhanced codebase maintainability with clear separation of concerns between authentication logic and protocol-specific implementations
  - Improved workspace architecture enabling future protocol extensions (gRPC, etc.) without introducing coupling
  - Updated documentation and build commands to reflect new crate structure

### Fixed - 2025-01-08
- Fixed REST service example authentication provider sharing issue
  - Resolved authentication failures after user registration due to provider instance isolation
  - Implemented SharedUserProvider wrapper to ensure consistent provider state across service components
  - Fixed issue where LocalUserProvider instance used for registration differed from SessionService instance
  - Authentication now works correctly for both pre-configured test users (admin/admin123, user/user123) and newly registered users
  - Enhanced code organization with proper provider lifecycle management

### Fixed - 2025-01-08
- Fixed REST API documentation schema display for optional fields showing as empty objects
  - Enhanced OpenAPI schema generation to convert `"type": ["string", "null"]` format to `"type": "string", "nullable": true"` for better explorer compatibility
  - Improved JavaScript schema processing in documentation UI to handle array type definitions (e.g., `["string", "null"]`)
  - Added recursive schema normalization for all nested objects and definitions
  - Optional fields like `email` and `display_name` now display as proper string input fields with meaningful examples
  - Both backend OpenAPI generation and frontend UI handling improved for comprehensive fix

### Enhanced - 2025-01-08
- Sprint retrospective update covering Static API Documentation Hosting & Explorer UI implementation
  - Documented strategic orchestration approach with successful role delegation (Architect → Backend Coder → UX Designer)
  - Noted seamless integration with existing `ras-rest-macro` patterns without breaking changes
  - Recognized custom API explorer UI success with tailored features
  - Highlighted zero-overhead implementation design for optional features
  - Identified opportunity for smaller proof-of-concept approach in future complex implementations

### Added - 2025-01-08
- Static API documentation hosting with embedded explorer UI for REST services
  - Complete static file hosting support integrated into the `ras-rest-macro` crate
  - Interactive API documentation with custom-built explorer UI
  - Embedded static assets using rust-embed for zero-dependency deployment
  - JWT authentication integration directly in the explorer interface
  - Responsive documentation UI with multiple theme support (default theme included)
  - Automatic OpenAPI spec serving at configurable endpoints
  - Optional feature with zero overhead when disabled - no performance impact
  - Enhanced REST service example showcasing documentation hosting capabilities
  - Configurable documentation paths and themes via macro parameters

### Enhanced - 2025-01-08
- Sprint retrospective process with enhanced development guidelines based on observed patterns
  - Added critical development rules based on sprint observation analysis
  - Five new rules: Test Early/Often, Specification First, Incremental Implementation, Macro Testing, End-to-End Validation
  - Enhanced Common Pitfalls with string type mismatches and move semantics guidance
  - Updated crate listings to include `ras-rest-macro` and build commands
  - Captured retrospective notes covering OpenRPC generation, registry setup, and REST macro implementation
  - Systematic approach to learning from development patterns and preventing recurring issues

### Enhanced - 2025-01-08
- REST service example now demonstrates complete local authentication integration with comprehensive security features
  - Full JWT-based authentication using `ras-identity-local` and `ras-identity-session` crates
  - Complete auth endpoints: user registration, login, logout, and user info retrieval
  - Role-based permission system with admin and user access levels (admin users inherit user permissions)
  - Two-phase authentication flow: LocalUserProvider for credential validation → SessionService for JWT issuance
  - Pre-configured test users (admin/admin123 with admin permissions, user/user123 with user permissions)
  - Environment-based configuration for JWT secrets, server host/port with secure defaults
  - Protected REST endpoints demonstrating permission-based access control in action
  - Comprehensive security implementation with Argon2 password hashing and session tracking

### Added - 2025-01-08
- REST macro crate implementation with comprehensive REST API generation capabilities
  - Complete `ras-rest-macro` procedural macro crate for type-safe REST endpoints with authentication integration
  - Supports all HTTP methods (GET, POST, PUT, DELETE, PATCH) with path parameters and request bodies
  - OpenAPI 3.0 document generation using schemars with configurable output paths
  - Permission-based access control with JWT authentication through AuthProvider integration
  - Generated service traits, builders, and axum router integration following JSON-RPC macro patterns
  - Example application demonstrating comprehensive REST service implementation
  - Full workspace integration with proper dependency management and testing infrastructure

### Added - 2025-01-08
- Kellnr registry notes for local crate publishing
  - Recorded the local registry URL `http://localhost:8000/api/v1/crates/`
  - Created comprehensive release checklist
  - Includes A-Z release process with dependency order management
  - All internal dependencies already properly configured with path + version

### Added - 2025-01-08
- Complete OpenRPC 1.3.2 specification types crate (ras-openrpc-types) with full type safety and validation
  - Comprehensive implementation of all OpenRPC specification types with serde serialization support
  - Ergonomic builder patterns using bon crate for fluent API construction
  - Extensive validation system for OpenRPC documents, method names, error codes, and component references
  - JSON Schema Draft 7 support with schemars integration for automatic schema generation
  - 142 comprehensive unit tests covering all types, builders, validation rules, and serialization scenarios
  - Complete documentation with working examples and doctest validation
  - Full workspace integration following established dependency patterns

### Added - 2025-01-08
- OpenRPC document generation support for jsonrpc_service macro
  - Added optional `openrpc` field to macro invocation for per-service control
  - Supports both default path (`target/openrpc/{service_name}.json`) and custom output paths
  - Generates complete JSON Schema definitions using schemars crate for all request/response types
  - Includes authentication metadata with OpenRPC extensions (`x-authentication`, `x-permissions`)
  - Added comprehensive test coverage and examples demonstrating all features
  - Updated JSON-RPC macro documentation with usage examples and requirements
  - Requires types to implement `schemars::JsonSchema` trait when OpenRPC generation is enabled

### Fixed - 2025-01-07
- Fixed JSON-RPC macro routing issue causing 404 errors when accessing service endpoints
  - Macro now properly uses the base_url parameter instead of hardcoding "/" routes
  - Services created with custom paths (e.g., "/rpc") now work correctly when nested in routers
  - This resolves 404 errors in the Google OAuth2 example and other JSON-RPC services

- Fixed Axum router nesting syntax in Google OAuth2 example
  - Corrected router nesting from incorrect .merge() syntax to proper .nest() method
  - API endpoints now correctly accessible at /api/rpc instead of returning 404 errors

- Simplified Google OAuth2 example environment configuration template
  - Streamlined .env.example with cleaner formatting and reduced verbosity
  - Removed redundant comments and example credentials that could cause confusion
  - Improved clarity of required vs optional configuration parameters

- Fixed Google OAuth2 field compatibility issue preventing successful authentication callbacks
  - Added serde field alias to support both "sub" (OpenID Connect/v2/v3) and "id" (Google v1) user identifier fields
  - Updated Google OAuth example to use v3 userinfo endpoint for better feature support
  - Maintains backward compatibility with existing OAuth2 provider configurations
  - Added comprehensive tests for both field formats and additional claims handling

### Added - 2025-01-07
- Complete OAuth2 provider implementation with Google OAuth2 support and comprehensive security features
  - OAuth2Client with PKCE (Proof Key for Code Exchange) support for enhanced security
  - In-memory state store with automatic expiration and cleanup mechanisms
  - Complete authorization flow handling including code exchange and user info retrieval
  - Custom user info field mapping for flexible OAuth2 provider integration
  - Comprehensive error handling with OAuth2-specific error types and detailed context
  - Full test suite covering PKCE generation, authorization URLs, state management, and security scenarios
  - HTTP timeouts and error handling for the provider client
- Enhanced JwtAuthProvider with Clone trait for improved service compatibility and architecture flexibility

### Added - 2025-01-07
- Google OAuth2 full-stack example application demonstrating complete authentication infrastructure
  - Interactive HTML/JS frontend with modern responsive design and real-time OAuth2 flow visualization
  - Complete Rust backend integration using Axum server with JSON-RPC API endpoints
  - Sophisticated permission system with role-based access control based on email domains and user attributes
  - Six different API endpoints showcasing permission-based access (user info, documents, admin, system status, beta features)
  - OAuth2 flow with PKCE, state validation, JWT session management, and error handling
  - Interactive API documentation with built-in testing capabilities and JWT token management
  - Comprehensive test suite covering permission logic and service compilation validation
  - Complete setup documentation with Google Cloud Console integration instructions

### Security - 2025-01-07
- Enhanced environment security with improved .gitignore patterns for secrets and credentials
  - Added comprehensive exclusion patterns for .env files, secrets directories, and OAuth2 credentials
  - Prevents accidental commitment of sensitive configuration data to version control
  - Includes protection for production, staging, and local environment configurations

### Documentation - 2025-01-07
- Updated Google OAuth2 example documentation and usage instructions
  - Added quick start guide with Google Cloud Console setup steps and environment configuration
  - Documented sophisticated permission system with role-based access control examples
  - Comprehensive API endpoint documentation with permission requirements and functionality descriptions
  - Added oauth2 provider status update from stub to implemented provider
  - Enhanced development commands with example application execution instructions
  - Added Common Pitfalls section documenting Axum router nesting syntax issues
- Updated sprint reflection documentation with Google OAuth2 full-stack implementation learnings and coordination insights
  - Added reflection on OAuth2 example routing fix process and systematic debugging approach
  - Documented lessons learned about testing end-to-end flows and examining generated code

### Security - 2025-01-07
- Enhanced authentication security in `ras-identity-local` with comprehensive attack vector protection
  - Fixed username enumeration vulnerability - consistent errors for non-existent users and wrong passwords
  - Implemented timing attack resistance using constant-time authentication with real Argon2 dummy hash
  - Added robust input validation for malformed payloads, empty credentials, and special characters
  - Enhanced concurrent authentication safety and brute force protection
  - Comprehensive security test suite covering 11 attack vectors including password spraying and timing analysis
- Updated authentication architecture documentation with detailed security measures
- Added security considerations and attack vector protection guidelines to development documentation

### Added - 2025-01-07
- Identity management system with pluggable authentication providers
  - `ras-identity-core`: Core traits for IdentityProvider and UserPermissions with default implementations
  - `ras-identity-local`: Local username/password authentication with Argon2 password hashing
  - `ras-identity-oauth2`: Initial OAuth2 provider framework for external-provider authentication
  - `ras-identity-session`: JWT-based session management with configurable secrets and permission lookup
- Two-stage authentication flow: identity verification followed by JWT session creation
- Permission system with UserPermissions trait enabling flexible RBAC patterns
- JwtAuthProvider implementing AuthProvider trait for seamless JSON-RPC integration
- Comprehensive test suite covering authentication workflows and permission assignment
- Design documentation and architecture patterns for identity management
- Workspace configuration updates to include identity management crates

### Fixed - 2025-01-07
- Resolved unused variable warning in JSON-RPC macro usage example

### Added - 2025-01-07
- Complete JSON-RPC library ecosystem with three core crates
  - `ras-jsonrpc-types`: Pure JSON-RPC 2.0 protocol types and utilities
  - `ras-jsonrpc-core`: Authentication and authorization framework with AuthProvider trait
  - `ras-jsonrpc-macro`: Procedural macro for generating type-safe RPC interfaces with axum integration
- Comprehensive test suite and integration tests for macro functionality
- Workspace-level dependency management with shared crate versions
- Example applications demonstrating JSON-RPC service implementation
  - basic-jsonrpc-service: Complete working example with authentication and multiple endpoints
  - Usage examples showing macro-generated service builders
- Enhanced project documentation and development guidelines
  - Updated crate organization patterns
  - Added development workflow instructions and dependency management guidelines
- Sprint reflection system for tracking development progress and learnings

### Added - 2025-01-06
- Initial project setup with Cargo workspace structure
- Created `ras-jsonrpc-macro` procedural macro crate foundation
- Added .gitignore for Rust and IDE artifacts
