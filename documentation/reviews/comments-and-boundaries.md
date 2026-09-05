Review date: 2026-09-05.

This records the initial review. The [refactor plan](refactor-plan.md) updates
priorities and ownership proposals against the subsequently merged security changes.

The main responsibility problems are concentrated in macro generation, the chat example,
and the shared explorer. Most crate boundaries are sensible.
We should split those files into cohesive modules before creating more crates.
Two package boundaries deserve separate attention: the explorer assets and bidirectional runtime adapters.

This pass inventories 175 Rust files and all 22 library crate manifests,
examines comment patterns across source, tests, and examples,
and checks the large HTML assets and their consumers.
The findings concern documentation and ownership; they are not a complete behavioral or protocol-conformance audit.
Line counts below describe the revision before comment cleanup and include comments and blank lines.

Comments now describe current behavior instead of audit labels, previous implementations,
feedback history, or coverage work. The cleanup also removes 225 selected narration lines
from macro generation, OAuth2, observability, and WebSocket code.
Comments explaining lock lifetimes, credential handling, feature resolution,
and other non-obvious constraints remain.
Short test scenario labels and examples that teach API usage remain useful.

Several corrections change the meaning of the documentation:

| Location | Correction |
| --- | --- |
| [HTTP transport](../../crates/core/ras-transport-core/src/lib.rs) | The WASM adapter buffers both bodies. Query-value serialization returns decoded pairs, which a separate helper form-encodes. |
| [Authorization helpers](../../crates/core/ras-auth-core/src/authorize.rs) | The full pipeline is shared by REST and file services. Other protocols reuse permission helpers. The helper itself cannot guarantee that its caller has not read the body. |
| [Caller](../../crates/core/ras-auth-core/src/lib.rs) | Construction requires trusted authentication results; it is not restricted to one helper, since public variants and `from_authenticated` exist. |
| [OIDC client](../../crates/identity/ras-identity-oauth2/src/client.rs) and [configuration](../../crates/identity/ras-identity-oauth2/src/config.rs) | Accepting an ID token requires a configured issuer. The constructor can panic and is documented accordingly. |
| [OAuth2 state](../../crates/identity/ras-identity-oauth2/src/state.rs) | The store description belongs on `InMemoryStateStore`, not on the preceding capacity constant. |
| [OpenRPC validation](../../crates/specs/ras-openrpc-types/src/validation.rs) | URL/email checks are shallow; the version helper checks only two numeric components. Single-name helpers do not validate collection uniqueness. These limits are explicit without changing behavior. |
| [JSON-RPC core](../../crates/rpc/ras-jsonrpc-core/src/lib.rs) | This crate is a runtime re-export facade; it does not own the authentication traits. |
| [Chat tests](../../examples/bidirectional-chat/server/tests/server_tests.rs) | The health fixture does not construct the application server. Auth lifecycle tests also wire a local service fixture. |

The first file splits should be:

| Priority | File and size | Proposed responsibility boundaries | Reason |
| --- | --- | --- | --- |
| 1 | [REST macro `lib.rs`](../../crates/rest/ras-rest-macro/src/lib.rs), 2,298 lines | `ast`, `parser`, `server` with handler/body extraction, routing, and version migration submodules. Keep the entry function and expansion orchestration in `lib.rs`. | Syntax, validation, and several kinds of emitted server code change independently. The existing client/spec/permission modules already establish this pattern. |
| 1 | [JSON-RPC macro `lib.rs`](../../crates/rpc/ras-jsonrpc-macro/src/lib.rs), 1,315 lines | `ast`, `parser`, `server`, and `dispatch` with version handling. | Parsing, HTTP policy, builder generation, and method dispatch share one file. Keep HTTP envelope handling separate from method dispatch. |
| 1 | [Chat server `main.rs`](../../examples/bidirectional-chat/server/src/main.rs), 2,663 lines; 1,696 before tests | Application state, chat operations, identity/session HTTP routes, persistence conversion, and router construction. Leave process setup in `main`. | This file owns an application rather than an entry point. Router construction should be callable by integration tests so fixtures need not duplicate it. Keep the existing persistence module as the storage owner. |
| 1 | [Explorer template](../../crates/rest/ras-rest-macro/src/api_explorer_template.html), 1,451 lines | CSS, schema/docs rendering, OpenAPI/OpenRPC normalization, request execution, and saved-request/history state. Assemble an embedded asset for consumers. | Styling, protocol interpretation, and application state are separate concerns. Preserve the generated explorer's self-contained delivery when splitting source assets. |
| 2 | [File server generator](../../crates/rest/ras-file-macro/src/server.rs), 1,048 lines | Support/trait types, upload handling and part validation, download handling, and route/auth glue. | Multipart state and limits dominate a file that also emits unrelated download and router code. |
| 2 | [REST OpenAPI generator](../../crates/rest/ras-rest-macro/src/openapi.rs), 823 lines | Schema collection/normalization and operation/document generation. | Most work is inside one large generator. Extract emitter functions as well as files; moving the whole function would not clarify ownership. |
| 2 | [JSON-RPC OpenRPC generator](../../crates/rpc/ras-jsonrpc-macro/src/openrpc.rs), 643 lines | Schema/reference handling, example generation, and method/document generation. | These transform different parts of the output contract. Compare duplicated normalization with REST after establishing local boundaries. |
| 2 | [Auth transport](../../crates/core/ras-auth-core/src/transport.rs), 1,088 lines; 739 before tests | Cookie configuration/emission, CSRF policy, credential extraction, and redaction, with a small transport facade. | Each has distinct invariants and tests. Preserve the current public re-exports. |
| 2 | [WASM UI](../../examples/wasm-ui-demo/src/lib.rs), 1,378 lines; 1,286 before tests | App state/service actions, login, statistics, task form/list, and dashboard composition. | UI sections have clear render functions but share a single large source file. |
| 3 | [WebSocket client](../../crates/rpc/bidirectional/ras-jsonrpc-bidirectional-client/src/client.rs), 1,465 lines; 772 before tests | Builder, connection/message driver, and facade; move the test module into a companion file. | The size is partly tests. Keep connection state, pending requests, and their lifecycle coordinated rather than scattering individual methods. |
| 3 | [WebSocket handler](../../crates/rpc/bidirectional/ras-jsonrpc-bidirectional-server/src/handler.rs), 1,206 lines; 551 before tests | Handler contract, WebSocket IO adapter, connection loop, and companion tests. | The transport adapter is independent of service callbacks. Moving tests is the lowest-risk first step. |
| 3 | [OAuth2 client](../../crates/identity/ras-identity-oauth2/src/client.rs), 1,143 lines; 548 before tests | HTTP adapter, PKCE/authorization request handling, ID-token claim validation, and tests. | Network errors and claim validation have different responsibilities; keep flow coordination in the client. |
| 3 | [Sessions](../../crates/identity/ras-identity-session/src/lib.rs), 894 lines; 522 before tests | Configuration, JWT codec/claims, session lifecycle, auth adapter, and tests. | Cryptographic token handling and active-session state can be reviewed independently within this crate. |
| 3 | [HTTP transport root](../../crates/core/ras-transport-core/src/lib.rs), 473 lines | Query serialization into `query`; path encoding into `path`. | A custom serde serializer is an independent concern from the transport contract. Network/test adapters already have modules and optional features. |

For tests, move large inline test modules before subdividing cohesive production types.
`ras-identity-local/src/lib.rs` is 743 lines, but only 199 precede its tests.
`ras-identity-oauth2/src/provider.rs` has 356 production/documentation lines and 347 test lines.
Neither needs a new crate.

The REST HTTP integration file is 1,377 lines and combines substantial service fixtures
with authentication, serialization, routing, and body-policy scenarios.
Group those scenarios into test modules with local support code.
Apply the same approach to the 727-line JSON-RPC HTTP suite and 809-line file-service end-to-end suite.
Keep fixtures in `tests/support` unless multiple crates truly need the same contract.
The `xm_feedback_*` filenames should eventually become behavioral names such as `http_contract`;
their headers now describe the tested behavior, but this pass leaves filenames intact.

The chat auth suite contains its own handlers and chat service implementation.
The smaller server suite builds a health-only router and ignores the supplied configuration.
After extracting application construction, have application integration tests call that shared constructor.
File splitting alone would otherwise preserve two versions of the behavior under test.

Some large files should stay cohesive. OpenRPC `schema.rs` has 936 lines,
of which 676 precede tests; `method.rs` has 827, with 558 before tests.
These own recognizable specification objects, their constructors/conversions, and validation.
Move tests first; split validation implementations only if navigating them remains difficult.
The chat TUI's 664-line `ui.rs` can be divided by screen when editing it,
while its 680-line server configuration file largely consists of configuration types and defaults.
Neither is as urgent as the macro roots or application entry point.
The OAuth2 demo's large HTML pages can share styles and browser session helpers,
but their markup is also teaching content, so preserve readable standalone examples.

At crate level, the decisions are:

| Crate | Rust lines under `src` | Decision |
| --- | ---: | --- |
| `ras-auth-core` | 1,757 | Keep; split HTTP auth responsibilities into modules. No runtime adapter forces a crate split. |
| `ras-identity-core` | 245 | Keep the identity and permissions contracts together. |
| `ras-observability-core` | 595 | Keep; its Axum header import couples a contract crate to a framework. Prefer an `http` dependency or an adapter module when addressing this boundary. |
| `ras-transport-core` | 1,274 | Keep; optional network, filesystem, and in-process adapters already isolate dependencies sufficiently. Split serialization modules first. |
| `ras-version-core` | 192 | Keep; the shared migration trait is only 15 lines before tests and has a clear independent purpose. |
| `ras-identity-local` | 743 | Keep; move tests. The provider's small implementation does not justify more crates. |
| `ras-identity-oauth2` | 3,002 | Keep; flow, provider, configuration, and state modules already form one identity adapter. Split the client internally. |
| `ras-identity-session` | 894 | Keep; JWT and active-session modules are sufficient. A separate JWT library needs a reuse requirement. |
| `ras-observability-otel` | 601 | Keep the concrete metrics/export adapter together. |
| `ras-file-core` | 394 | Keep; upload/download runtime contracts are cohesive. |
| `ras-file-macro` | 3,191 | Keep the macro crate; split server generation internally. |
| `ras-rest-core` | 289 | Keep the REST runtime facade and response/error contracts together. |
| `ras-rest-macro` | 4,103 | Keep parsing and generation in this macro crate; extract shared explorer ownership separately. |
| `ras-jsonrpc-bidirectional-client` | 3,172 | Keep; native and WASM adapters serve the same client contract and already have separate modules. |
| `ras-jsonrpc-bidirectional-macro` | 1,575 | Keep; parser/client/server/permission concerns already have useful boundaries. |
| `ras-jsonrpc-bidirectional-server` | 3,582 | Keep; connection, manager, service, router, upgrade, and handler modules form one runtime. Refine `handler`. |
| `ras-jsonrpc-bidirectional-types` | 1,350 | Separate concrete senders from shared contracts; see below. |
| `ras-jsonrpc-core` | 199 | Keep as the stable generated-code facade. Its implementation is re-exports, not an oversized domain crate. |
| `ras-jsonrpc-macro` | 2,700 | Keep the macro crate; split parsing/server generation and stop importing assets from its sibling's source tree. |
| `ras-jsonrpc-types` | 359 | Keep the protocol envelope types independent. |
| `ras-openrpc-types` | 6,895 | Keep; 17 modules already divide specification objects. Total crate size alone is not a reason to separate the schema model. |
| `ras-permission-manifest` | 398 | Keep the transport-free tooling artifact independent. |

The shared explorer is the strongest new-package candidate.
[`ras-jsonrpc-macro/src/static_hosting.rs`](../../crates/rpc/ras-jsonrpc-macro/src/static_hosting.rs)
uses `include_str!` to reach into `ras-rest-macro/src/api_explorer_template.html`.
That ownership is absent from Cargo's dependency graph and depends on the workspace layout.
Move the shared embedded asset into a small ordinary library crate consumed by both macros,
or establish another explicit packaged-asset boundary.
Keep REST and JSON-RPC route generation in their respective macro crates.
The extraction should verify independently packaged builds, not only workspace compilation.

The bidirectional types crate mixes wire messages and shared traits with a concrete
`WebSocketMessageSender` backed by Tokio and Tungstenite.
Move concrete senders to an adapter boundary; retain wire types and genuinely shared contracts.
There are no in-tree uses of `WebSocketMessageSender` outside its own implementation and tests,
so an existing runtime crate may be a sufficient destination.
Create a dedicated adapter crate only if it must serve independent consumers.
Audit pending-request channels and error types too before claiming the contracts are runtime-free.
This changes public import paths and needs an explicit compatibility/versioning decision.

Do not create a general macro-utilities crate merely to collect similarly named helpers.
Permission metadata and schema handling repeat across macro crates,
but shared ownership should follow equivalent semantics and shared tests.
The explorer already has an actual shared consumer, which makes its boundary concrete.

No files, modules, or crates were split in this pass.
Validation checks that all non-comment, nonblank Rust lines match the original revision,
and `git diff --check` checks whitespace.
Offline documentation builds with all features pass for `ras-auth-core`,
`ras-transport-core`, `ras-identity-oauth2`, `ras-openrpc-types`, and `ras-jsonrpc-core`.
No behavior tests were added or run for the comment-only changes.
