Refactor plan, 2026-09-05. Based on `84346d6`, which includes the comment-only
changes in PR #27 on top of `a2943d2` (the September security changes).
Execution: one MR, with sequential verified checkpoints. The
[execution record](refactor-progress.md) maps completed changes to their validation.
Each checkpoint is tested and committed before the next extraction begins.

This plan supersedes the priorities and size estimates in the earlier
[boundary review](comments-and-boundaries.md). The required work is tracked in the execution record.

We should start with macro ownership, then separate runtime policy from lifecycle code,
and finally simplify application wiring and examples.
The shared explorer is the one new crate justified immediately by existing consumers.
Other crate extractions need a concrete dependency benefit before proceeding.

The default is to preserve public APIs, public module paths, feature behavior,
generated identifiers, serialized data, HTTP responses, and protocol ordering.
Existing public paths can re-export moved types.
Keep internal modules private and expose only the minimum visibility needed between siblings.
Breaking package moves are a separate decision described below.

Each checkpoint should have a single responsibility change.
Move existing code and tests first; isolate any necessary logic changes in a separate checkpoint commit.
An extraction is finished when the new owner is clear and callers depend on its narrow interface.
Moving a thousand-line function into another file does not meet that condition.

Use roughly 200–500 production lines as a navigation guideline, not a quota.
Review files above about 700 production lines for multiple responsibilities.
Tests, documentation examples, and large cohesive specification objects explain some exceptions.
Avoid both a universal `utils` module and one-file-per-method fragmentation.

The first tranche consists of these independently reviewable checkpoints:

| checkpoint | Scope | Dependency | Completion criteria |
| --- | --- | --- | --- |
| 1 | REST macro model and parser | Comment PR as the working baseline | `lib.rs` no longer defines/parses the service language. Existing valid inputs, errors, spans, and generated names remain stable. |
| 2 | REST server generation | 1 | Builder/router emission, request extraction, and canonical/versioned handlers have distinct owners. The macro root only parses, orchestrates, and reports errors. |
| 3 | JSON-RPC macro model and parser | Baseline; use the naming conventions established by 1 | Separate language model and parsing without sharing a new generic parser with REST. |
| 4 | JSON-RPC server generation | 3 | Separate HTTP envelope/auth policy from method dispatch and version migration. |
| 5 | Shared explorer package | Baseline | Both macro crates obtain the same template from an explicit Cargo dependency; neither reads the other's source tree. |
| 6 | WebSocket handler tests and policy ownership | Baseline | Move the large test module and shared policy/accounting types out of `handler.rs`; retain the current connection loop and checked subscription mutation path. |

Implement 1 and 2 first to establish a useful macro structure.
Then finish 3 and 4. checkpoint 5 is independent of the parser/server extractions,
and checkpoint 6 establishes the boundary needed by later WebSocket work.
Verify and commit each completed checkpoint before starting another that changes the same files.
Keep the plan and review notes out of PR #27.

For the REST macro, use this responsibility map:

| Owner | Responsibility |
| --- | --- |
| `lib.rs` | Public macro documentation, proc-macro entry point, error conversion. |
| `ast.rs` | Service, endpoint, version, permission, parameter, and documentation models. May use `syn` types; must not depend on emitters. |
| `parser/` | Service fields, endpoint/version grammar, paths/queries, doc attributes, and syntax diagnostics. |
| `expand.rs` | Select and assemble client, server, spec, permission-manifest, and explorer output, including feature gating. |
| `server/mod.rs` | Assemble the server expansion and generated trait/builder. Extract a builder module if this remains large. |
| `server/routes.rs` | Route and extractor registration for canonical and versioned endpoints. |
| `server/request.rs` | Request-part types, argument order, body limits/content type, and body decoding. |
| `server/handlers.rs` | Canonical invocation, authorization sequencing, tracking, and response conversion. |
| `server/versioned.rs` | Adapt versioned request parts and responses around the canonical handler contract. |

Keep the existing `client`, `permissions`, `openapi`, and `static_hosting` owners.
Do not unify canonical and versioned handlers while moving them;
first make their differences visible through explicit inputs and outputs.
Auth must still precede body consumption and the relevant request extraction.
Preserve the newer raw-request handling and error classification from the security changes.

Use the same `ast`, `parser`, and `expand` conventions for JSON-RPC.
Its server modules should own builder/router construction, HTTP envelope processing,
and method dispatch respectively.
Dispatch owns method names, parameter decoding, permission decisions, and migration adapters.
The HTTP layer owns content-type/body limits, envelope errors, request-level credential resolution,
and the HTTP status mapping. Preserve optional-auth downgrade behavior across that boundary.
Similarity to REST does not imply that their auth pipelines can be merged.

For the explorer, create an ordinary library crate such as
`crates/specs/ras-api-explorer-assets` with a small embedded-template API.
Place it in the existing `crates/specs/*` workspace group because it serves the API specifications.
It should have no Axum, Tokio, `syn`, or `quote` dependency.
REST and JSON-RPC retain their own route generation, authorization gates,
configuration serialization, and escaping.
The first checkpoint moves the template intact and preserves its bytes.

In a subsequent explorer checkpoint, divide source assets into CSS, markup, schema/document rendering,
OpenAPI/OpenRPC normalization, request construction/execution, and local saved/history state.
Assemble them at compile time into the same self-contained HTML response.
Prefer fixed-order concatenation with an explicit initializer; do not introduce a frontend framework,
runtime asset server, or JavaScript bundler solely for this split.
Keep the JSON configuration placeholder and its escaping contract explicit.

checkpoint 6 needs more care than the original review suggested.
The WebSocket handler now has 2,060 lines, including about 1,216 lines of inline tests.
It defines subscription limits/accounting that `connection.rs` imports,
while the handler also depends on connection state.
Move `SubscriptionLimits`, `SubscriptionAccounting`, and `SubscriptionPolicy`
into a shared `subscriptions` module. Re-export existing public paths.
Keep `ConnectionContext::subscribe` as the checked mutation entry point;
move policy ownership without creating an alternate path around it.

A follow-up WebSocket checkpoint can separate the handler contract, socket IO adapter,
and revalidation/keepalive configuration from the connection loop.
Use `handler/tests/` for lifecycle, wire errors, revalidation, subscription limits,
and keepalive scenarios with local shared fixtures.
The loop continues to own timer/select ordering, cancellation, sending,
and disconnect cleanup. Do not turn each event into an independent task.
Keep the final subscription check immediately before the outbound socket write.

The second tranche contains these bounded work packages.
Each row is one checkpoint unless the split column explicitly describes a sequence.

| Work package | Proposed owners and checkpoint boundary | Validation focus |
| --- | --- | --- |
| File server generation | `server/types`, `upload`, `download`, `routes`, and local auth glue. Keep upload part dispatch/limits together; split part generation underneath `upload` if necessary. | Streaming, aggregate/part limits, cancellation/draining, filename rejection, auth, and multiple macro invocations. |
| REST OpenAPI generation | After checkpoint 2, split schema collection/normalization from operation/document emission. Extract smaller emitter functions inside those owners. | Compare generated JSON semantics, references, nullable shapes, permissions, versioned routes, and explorer rendering. |
| JSON-RPC OpenRPC generation | After checkpoint 4, separate schema/reference transforms, examples, and method/document emission. Separate checkpoint from OpenAPI. | Generated JSON, method names, references, permissions, examples, and explorer rendering. |
| HTTP auth transport | Keep a `transport` facade over `cookie`, `csrf`, `credential`, and `redaction` modules. Tests follow the relevant invariants. | Credential precedence, ambiguous inputs, cookie attributes, all CSRF modes, and custom sensitive headers. No policy changes. |
| Sessions | `config`, `claims`, `jwt`, `session`, `auth`, and companion tests. Current root is 1,397 lines, with about 678 lines of tests. | Preserve secret/config validation, algorithms, issuer/audience defaults, expiry, active-session caps, revocation, permission snapshots, and cleanup. Keep signing/verification together in `jwt`. |
| OAuth2 client | Separate HTTP transport, PKCE, authorization-request policy, and ID-token validation; `client` coordinates the flow. Keep state storage and identity mapping with their existing owners. | HTTPS requirements, reserved parameters, binding/nonce/state consumption, issuer/audience/subject checks, token endpoint failures, and redaction. Move provider/client tests with their owners. |
| HTTP transport helpers | Move query serde collection into `query` and path encoding into `path`; preserve root exports. | Repeated keys, enum renames, form encoding, path escaping, and generated client round trips. Keep optional adapter features intact. |
| Observability dependency | Replace the core crate's `axum::http::HeaderMap` import/dependency with `http::HeaderMap`. Keep the existing public signatures and extractor behavior. Separate checkpoint from transport helpers. | Core and OTEL tests plus dependency inspection showing that core no longer directly depends on Axum. |
| WebSocket client | Move tests first, then builder and message driver into child modules behind the existing facade. Keep native/WASM adapters separate. | Handshake completion, request cleanup, subscription dispatch, header/subprotocol auth, native builds, and WASM builds. Preserve lock scopes and callback lifetimes. |

Do not move security decisions into a generic shared helper merely because branches look similar.
Any deduplication of permission or schema code comes after these local boundaries are stable,
with evidence that both callers require the same semantics.

The application tranche makes examples easier to follow and tests more representative:

| Work package | Scope and checkpoint boundary | Completion criteria |
| --- | --- | --- |
| Chat library and application construction | First move state, chat operations, auth handlers, persistence conversions, and router construction into the existing library. Keep environment loading, tracing setup, listener binding, and serving in `main`. | A constructor accepts explicit configuration/dependencies and returns the assembled application without binding a socket or configuring global logging. Existing behavior and tests remain intact. |
| Chat integration fixtures | Follow the construction checkpoint. Replace duplicated auth/chat handler implementations in integration tests with the actual application constructor. | Login, registration, permissions, persistence-backed state, and WebSocket lifecycle exercise the production wiring. Remove the health-only stand-in where application startup is the intended subject. |
| WASM demo | Move state/service actions into an app module and renderers into login, statistics, task form/item/list, and dashboard modules. | Preserve signal ownership, event lifetimes, requests, and rendered behavior. Build the WASM bundle and exercise login, create/update/list, and failure states. |
| Secondary examples | Separate optional checkpoints for TUI screen renderers and OAuth2 demo styles/browser session helpers. | Existing TUI behavior and standalone demo flows remain readable and usable. No redesign. |

For chat, keep one owner of shared state and have the large generated service trait implementation
delegate to cohesive operations such as rooms, messages, profiles, and typing.
Do not create separate stores or locks just to divide methods between files.
Keep persistence IO in its existing module; isolate DTO/domain conversion from file access.
The application constructor should also retain handles needed for cleanup if it creates background work.
Integration tests supply temporary persistence locations and explicit configuration.
They must not start setting process-wide environment variables or logging subscribers.

Test-file cleanup should follow the owner being refactored rather than become one workspace-wide checkpoint.
For the large REST/JSON-RPC/file integration suites, keep the test target name where practical
and split scenario modules underneath it, with a local `support` module for fixtures.
Record discovered test names/counts before and after moving tests so none silently disappear.
Rename the two `xm_feedback_*` targets to behavior-oriented names in a dedicated small commit,
updating references in scripts, CI, and docs if any exist.
Move the large local-identity test module without splitting its small provider implementation.

OpenRPC `Schema` and `Method` remain cohesive objects.
Move their tests into companion files when convenient; do not split their model into new crates.
Likewise, keep chat configuration types together unless repeated navigation reveals a useful grouping.
These are optional cleanup items, not prerequisites for the higher-value work.

The bidirectional types crate needs a deliberate package decision.
Moving `WebSocketMessageSender` into the existing client/server runtime cannot preserve its old
re-export through the types crate: that would create a Cargo dependency cycle.
Moving it into a new adapter crate has the same issue if the types crate re-exports it.
Also, `ConnectionManager` exposes Tokio oneshot senders, so moving the concrete sender alone
does not make the contract runtime-independent.

Default plan: separate wire models, manager contracts, sender contracts, and concrete adapters
into internal modules; retain current public paths and dependencies for this cycle.
Defer the package move to a breaking release with a migration guide.
At that point, move concrete senders into an existing runtime crate if there is one real consumer,
or a dedicated adapter crate if independent users need it.
Removing Tokio from contracts would be a further API redesign and is outside this refactor.
This default can be changed explicitly before planning that release.

Validation uses the repository's existing CI contract rather than a new refactor-specific harness.
Before each package's first move, establish that its relevant suites pass on the chosen base.
Run focused checks during edits and the existing required CI jobs on the resulting checkpoint.

| Change | Required evidence |
| --- | --- |
| All Rust moves | Format and Clippy checks, affected package tests, downstream compilation, docs/doctests, and inspection of public paths/visibility. Keep the current no-retry nextest policy. |
| Macro generation | Existing HTTP/end-to-end, error, versioning, optional-auth, and multiple-invocation suites. Exercise server-only, client-only, no-default-feature, and consumer feature-gated builds from CI; all-features alone is insufficient. Add a small contract fixture only where a moved boundary lacks coverage. |
| Spec emission | Compare representative generated documents before/after as parsed JSON, ignoring irrelevant object-key ordering. Preserve references, operation names, security declarations, and response shapes. |
| Explorer package/assets | REST and JSON-RPC Playwright suites, existing XSS/token-storage/explorer tests, byte comparison for the initial asset move, and packaged-build checks. |
| WebSocket server | Unit suites plus `custom_manager_limits`, `transport_limits`, and `manager_unit`; preserve revocation, admission/subscription accounting, keepalive, and slow-client behavior. Use deterministic synchronization, not added sleeps. |
| WebSocket client/UI | The repository's native/WASM feature matrix; UI bundle build and targeted interaction checks. Native tests cannot establish browser callback behavior. |
| Chat fixtures | Tests visibly call the real constructor; health/auth paths and a WebSocket session work through that router. Keep isolated tests for config and persistence. |

For normal package checks use `cargo nextest run -p <package> --all-targets --all-features --locked`
and `cargo test --doc -p <package> --all-features --locked`, with downstream suites as listed above.
Use the exact feature combinations in `.github/workflows/ci.yml` for macro and WASM validation.
The explorer suite runs with `npm --prefix tests/playwright test` after its documented setup.

The explorer package needs more than workspace compilation.
Inspect `cargo package --list` to confirm all embedded source assets are present.
Unpack the package archives and compile the macro consumers without access to sibling source directories.
Resolve unpublished workspace dependencies through a temporary local registry or explicit test patches;
publishing is not needed for verification and is not part of this plan.

The MR description should name the new owner, what moved, and the contract evidence.
Report any baseline test failures separately from refactor regressions.
A failed behavioral check means investigate the extraction; do not rewrite expected output merely to pass.
No feature redesigns, dependency upgrades, or broad test-framework changes are bundled
with these responsibility refactors. The existing WASM task-details CSS-token panic was
fixed in a separate checkpoint before its module extraction, with a browser regression test.
