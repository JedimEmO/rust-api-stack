Round 1, plan review for authored examples, error examples and handler errors in the generated API documents.

Read documentation/reviews/api-examples-plan.md, the plan.
Versioning rules are in VERSIONING.md.
The code it changes:
- crates/rpc/ras-jsonrpc-macro/src/ (ast.rs, parser.rs, openrpc/, server/dispatch.rs, server/http.rs, client.rs, expand.rs)
- crates/rest/ras-rest-macro/src/ (ast.rs, parser.rs, openapi/, server/handlers.rs, expand.rs)
- crates/rpc/ras-jsonrpc-types/src/lib.rs (JsonRpcError, error_codes)
- crates/core/ras-transport-core/src/error.rs (TransportError)
- crates/rpc/bidirectional/ras-jsonrpc-bidirectional-macro/src/ (server.rs, client.rs),
  crates/rpc/bidirectional/ras-jsonrpc-bidirectional-server/src/ (router.rs, error.rs),
  crates/rpc/bidirectional/ras-jsonrpc-bidirectional-client/src/error.rs
- crates/specs/ras-api-explorer-assets/src/assets/ (specs.js, forms.js, schema-render.js)
Existing spec and sanitization tests: crates/rpc/ras-jsonrpc-macro/tests/ and crates/rest/ras-rest-macro/tests/
(http_integration/specs.rs, error_sanitization_test.rs, explorer_test.rs).
schemars is 1.0.0-alpha.20 (workspace Cargo.toml).

The larger goal: generated OpenRPC/OpenAPI documents that describe calls the server actually accepts,
with examples that cannot drift from the Rust types,
and JSON-RPC handlers able to return errors a client can act on, without weakening error sanitization.

Your role is to keep this plan sound and on track with that goal, and to break it before we build it.
This is a design review; do not edit files. Try in particular:
- The by-name parameter split. Which request types does schemars 1.0.0-alpha.20 emit in a shape
  the plan's "plain object" test gets wrong: `#[serde(flatten)]`, `rename_all`, `deny_unknown_fields`,
  `Option` fields with `skip_serializing_if`, generic types, newtype structs, types reached through `$ref`?
  Does a by-name call built from the descriptors decode on the server for each of them?
- Feature gating and compile cost. Where is the OpenRPC/OpenAPI generation code compiled
  (which features, which targets, including wasm clients)? Does requiring `Serialize` on example values,
  or emitting example expressions, break a build that has no `openrpc`/`openapi` or no server feature?
- Error passthrough security. Walking `source()` to find a `JsonRpcError`: can an error the handler did not
  intend to expose reach the client (for example from a library that returns a `JsonRpcError` internally,
  or the bidirectional client error surfacing inside a server handler that calls another service)?
  Is the reserved-range rule enough, and is it enforced in every path (HTTP dispatch, versioned/migrated
  methods, bidirectional)?
- The HTTP status mapping in crates/rpc/ras-jsonrpc-macro/src/server/http.rs for error codes:
  what status does a handler-chosen application code get, and does anything else key off the code?
- Breaking-change analysis: is the version table right, and is anything missing
  (generated client signatures, explorer config, the permission manifest, examples/ crates)?
- Checkpoint 1: is validating the current output against the OpenAPI 3.0 schema and the OpenRPC
  meta-schema realistic given schemars emits JSON Schema 2020-12 constructs, or does it balloon?
  Propose a scope that keeps it bounded.
- What the planned tests would not catch.

My open questions:
1. Serializing an example can fail while the document is generated (it is also generated at runtime to serve
   openrpc.json/openapi.json). Panic, skip the example with a log, or something else?
2. Should handlers be allowed built-in codes other than -32602 (for example -32001 when a handler does its own
   auth check)? Or should the rule be stricter?
3. Is putting examples inside the macro block the right place, versus a function path or an attribute on
   the request type, given macro size and compile errors?
4. Is `x-ras-params-raw` with the single `params` descriptor the right fallback, or should non-object
   request types be rejected at compile time when openrpc is enabled?

Answer with the verdict first: agree, agree with small changes, or changes needed.
Then each finding: where (file:line or plan section), the failure with concrete inputs,
and a change concrete enough to apply without another round. Mark each a blocker or a follow-up.
Answer the open questions. No praise, no summary of the plan.
