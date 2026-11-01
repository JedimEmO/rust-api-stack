# Security review of 2026-09-30

A review of revision `15404c3` found twelve issues, three of them high severity.
It reproduced them with tests and changed no code.
`report.json` holds the findings, `dependency-audit.json` the advisory scan,
and `reproductions.log` the failing test run.

Before the first crates.io release, each finding was checked again against the current code.
All twelve still held, and all are fixed.
The reproducing tests run in CI as regression tests.

| Finding | Severity | Fix | Guarding tests |
| --- | --- | --- | --- |
| F01 Identity-provider namespaces lost in sessions | High | User IDs are `<provider_id>:<subject>`; provider IDs may not contain `:` (`8122f4a`, `b7e95c3`) | `ras-identity-session/tests/provider_scoped_users.rs` |
| F02 Forced WebSocket disconnect leaves the connection live | High | A disconnect closes the connection and cancels its work (`028033c`) | `ras-jsonrpc-bidirectional-server/tests/connection_termination.rs` |
| F03 Token request redirects replay the client secret | High | Token and userinfo requests do not follow redirects (`08bfdff`) | `ras-identity-oauth2/tests/client_safety.rs` |
| F04 `OAuth2Client` skips the HTTPS check | Medium | Every client entry point validates the configuration (`08bfdff`) | `ras-identity-oauth2/tests/client_safety.rs` |
| F05 Low-level OAuth flow unbound by default | Medium | The default flow is bound to the initiating browser (`08bfdff`) | `ras-identity-oauth2/tests/client_safety.rs` |
| F06 Slow WebSocket peers stall broadcasts and re-validation | Medium | Full queues fail fast; lifecycle checks take priority (`028033c`) | `connection_termination.rs`, `manager_unit.rs` |
| F07 Native client deadlocks on a normal close | Medium | Close handling no longer re-takes a held lock (`028033c`) | `ras-jsonrpc-bidirectional-client/tests/native_peer_close.rs` |
| F08 Dot-only path parameters change the endpoint | Medium | Clients and the explorer refuse `.`, `..` and empty segments (`8f56bc5`, `17c888e`) | `ras-transport-core/tests/security_regression.rs`, `ras-rest-macro/tests/path_param_encoding.rs`, the REST explorer Playwright tests |
| F09 `Debug` output shows credentials | Medium | Credentials are redacted in every reported type (`08bfdff`, `8122f4a`, `8f56bc5`, `028033c`) | unit tests next to each type |
| F10 Vulnerable rustls in `Cargo.lock` | Medium | rustls 0.23.46; `cargo deny` runs in CI | `Supply chain policy` CI job |
| F11 Exceeding the upload limit is not terminal | Low | The first read error sticks and aborts the upload (`8f56bc5`) | `ras-file-macro/tests/upload_part_errors.rs` |
| F12 Unbounded WASM inbound queue | Medium | The queue is bounded by count and size (`028033c`) | unit tests in `ras-jsonrpc-bidirectional-client/src/inbound.rs` |

The three hardening items in the report are done as well:
the local password-hashing permit survives cancellation (`8122f4a`),
a dropped WebSocket call frees its pending entry (`028033c`),
and generated clients collect at most 16 MiB of a response body by default (`8f56bc5`).
Rate limits on login endpoints remain a deployment concern.
