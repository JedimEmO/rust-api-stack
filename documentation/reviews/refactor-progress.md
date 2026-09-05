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
