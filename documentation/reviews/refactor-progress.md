Single-MR responsibility refactor. Base: `84346d6` (PR #27).

The accepted plan is executed sequentially on `refactor/responsibility-boundaries`.
Every checkpoint below passed its affected-package gate before the next extraction.
The gate includes formatting, nextest, doctests, Clippy with warnings denied,
and additional feature/consumer checks where recorded. Raw logs are in
`/tmp/ras-refactor-logs` for this workspace session.

Baseline: workspace nextest ran 926 tests: 924 passed, two SIGSEGV failures,
one skipped. The failures were `ras-identity-session::permissions_are_frozen_into_the_token_snapshot`
and `ras-identity-local::test_duplicate_user_is_rejected`, before refactoring.
Identity work must investigate these failures before its checkpoint can pass.
REST baseline: 61/61 passed.

| Step | Change | Verification |
| --- | --- | --- |
| 1 | REST model and parser | 61 tests, 1 doctest; Clippy; no-default/server/client macro builds; no-default/server native and client WASM `rest-api` builds. |
| 2 | REST expansion, routing, request extraction, canonical/versioned handlers | 61 tests, 1 doctest; Clippy; all three macro feature modes; server native and client WASM consumer builds. |
| 3 | JSON-RPC model and parser | 59 tests; doctests (1 pre-existing ignored example); Clippy; all macro feature modes and no-default/server/client-WASM `basic-jsonrpc-api` builds. |
| 4 | JSON-RPC builder, HTTP envelope/auth policy, method/version dispatch | 59 tests; doctests; Clippy; all macro feature modes; native-server and WASM-client consumer builds. |
| 5 | Shared explorer assets crate | Original template SHA-256 preserved; 120 macro tests; docs/Clippy/features; 11/11 browser tests (baseline also 11/11); asset and macro packages created offline, unpacked macro builds pass using local dependency patches. |
| 6 | WebSocket subscription policy/accounting and handler test organization | 112 server/macro tests including all 29 moved handler tests; docs and Clippy; chat server consumer build. Checked mutation and egress checks unchanged. |
| 7 | WebSocket handler contract, socket IO, and lifecycle configuration | 112 server/macro tests; docs and Clippy. Public handler paths re-export moved types; connection loop remains together. |
| 8 | Explorer markup, styles, rendering, state, and request assets | Assembled HTML remains byte-identical (60,172 bytes); 120 tests; docs/Clippy/macro features; 11 browser tests; packaged and unpacked macro builds. |
| 9 | File-service types, uploads, downloads, routes, and auth generation | Baseline and result: 49 tests; docs/Clippy; no-default/server/client macro checks; native and WASM API consumer checks. |
| 10 | OpenAPI schema collection/normalization and operation emission | 61 tests, doctest, Clippy/features, 11 browser tests. 64 original and extracted document samples produce the same four JSON variants: schema titles already vary with HashMap insertion order. No output policy changed. |
| 11 | OpenRPC schemas/references, examples, and methods | 59 tests, docs/Clippy/features, 11 browser tests; three baseline JSON documents equal all 64 extracted samples each. |
| 12 | HTTP credential, cookie, CSRF, and redaction policy behind the transport facade | Auth baseline 39 tests; result 208 auth/HTTP macro tests; all 24 transport tests retained under scenario owners; docs/Clippy/macro feature matrix. |
| 13 | Session config, claims, JWT signing/verification, lifecycle, and auth adapter | Before and after: 40 identity tests pass (1 existing skipped), including both original crashing cases; docs/Clippy and chat consumer build. Original full-workspace SIGSEGV cause remains unreproduced. |
| 14 | OAuth2 HTTP transport, PKCE, authorization parameters, ID-token validation, and companion tests | Baseline and result: 55 tests; all 19 moved client tests retained; docs and Clippy. Timeout construction moved into the transport owner. |
| 15 | HTTP query serialization and path encoding | Baseline and result: 36 tests; docs/Clippy; no-default build; all three generated API clients compile for WASM. Root exports preserved. |
| 16 | Observability core depends directly on `http`, not Axum | Baseline and result: 37 core/OTEL tests; docs/Clippy; depth-one dependency tree contains `http` and no Axum. |
| 17 | WebSocket client test scenarios | Baseline and result: 53 tests (18 moved client cases); doctest/Clippy; WASM build with the CI no-default/wasm feature combination. Pre-existing extra blank line formatted. |
| 18 | WebSocket client builder and message driver | 53 tests, doctest/Clippy; explicit native and WASM CI builds. Existing driver/heartbeat ordering and lock scopes retained. |
| 19 | Chat library application constructor, state, operations, auth, and persistence conversions | Baseline and result: 28 tests; docs/Clippy. Ten service methods delegate to cohesive operation owners sharing the existing state/locks. Main retains environment, tracing, listener and serving; constructor accepts explicit identity storage and development seeding and returns router/session/manager handles. |
| 20 | Chat fixtures call the production application constructor | 29 tests (28 retained + router-level authenticated WebSocket/persistence/cleanup test); docs/Clippy. Removed duplicate auth/chat implementations and health-only stand-in. Assertions now reflect existing production 201 registration, 400 malformed input, and JSON health contracts. |
| 21a | Fix existing task-details CSS-token panic before UI extraction | Seven Rust tests, Clippy, WASM bundle and browser interaction test pass. Browser baseline had panicked on one space-separated `class` token; split it into two calls. New browser test covers login failure/success, list/create/complete/delete and failed-create state. cdylib has no doctest target. |
| 21 | WASM UI app state/actions and component renderers | Seven Rust tests, Clippy, WASM bundle and Chromium interaction test pass after extraction. Native cdylib doctests are not applicable. Signal/event ownership unchanged. |
| 22 | Bidirectional wire/connection models and sender contracts/adapters | Type baseline 14 tests; result 158 type/client/server tests; docs/Clippy and WASM client build. Existing root and sender-module exports and dependencies preserved. |
