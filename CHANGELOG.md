# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added - 2026-06-10
- Service-coms and gateway extension (issues #12–#15), shipped as new 0.1.0 crate families:
  - `ras-authorization-token`: shared RAS token claims (`ras_web_session`, `ras_internal_access`, `ras_gateway_access` families), ES256/EdDSA/HS256 signing, `kid`-based key rotation with JWKS publication, and a strict validator (asymmetric-only algorithm allowlist by default, issuer/audience/token-type pinning, key-type-confusion guard, clock-skew-aware expiry).
  - `ras-integration-core` (#12): outbound token framework — pluggable `TokenSource`s with token families, bounds-checked caching `TokenManager` (family/subject/audience/scopes/config-version cache keys, early refresh, concurrent-refresh dedup), `GrantStore` trait, `SecretString` redaction, and capability-scoped `AuthorizedHttpClient`s over `ras-transport-core` with exact-host outbound validation and no automatic replay.
  - `ras-integration-oauth2`: OAuth2/OIDC token source (refresh-token flow with grant scope subset checks and transactional rotation persistence, client credentials with audience forwarding, typed `ConsentRequired` errors) plus a PKCE S256 `ConsentFlow` with single-use, expiring, fully-bound state.
  - `ras-authorization-core` (#13, embedded mode): RAS-native authorization control plane — service registry, audience-scoped grants and roles, permission-manifest import with unknown-permission rejection, pluggable `ServiceIdentityVerifier` (constant-time static-secret dev verifier included), fail-closed internal token issuer with topology policy enforcement, JWKS/key rotation, append-only audit events, embedded axum authority routes, and `RasTokenAuthProvider` for downstream validation through existing generated services.
  - `ras-integration-ras`: `RasInternalTokenSource` bridging the two — obtains internal service tokens from the authority (in-process `EmbeddedAuthority` or HTTP `HttpAuthority`), never minting locally.
  - `ras-authorization-gateway` (#14): optional token-narrowing reverse proxy — local web-session validation via JWKS, deterministic longest-prefix routing, single-audience derived tokens that never outlive the session, header hygiene, streaming bodies, generated-profile consumption, and fail-closed WebSocket upgrades (v1).
  - `ras-topology-core` + `ras-topology-macro` (#15): `ras_topology!` compile-checked service graphs with build-time validation (audience uniqueness, route conflicts, exposure rules, manifest-checked edge permissions) emitting deterministic authorization-policy, gateway-profile, and Mermaid artifacts consumed by the authority and gateway.
  - `examples/authorization-demo`: end-to-end demo wiring topology, embedded authority, internal service calls through generated clients, and the gateway in front of two generated REST services, with a full in-process integration test suite.
- New mdBook chapters: Service-To-Service Auth, Outbound Integrations, The Auth Gateway, and Topology.

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
