Round 3, after the build. The plan documentation/reviews/api-examples-plan-3.md is built;
the change is the commit range 15404c3..0297c76 on branch feat/api-doc-examples.
Review the commits (git log / git show over that range), not the working tree,
which may change while you read. This is a review; do not edit files.

The design changed during the build; the plan's section "Build notes" lists every change. The main ones:
- Schema assembly uses one schemars generator per contract with subschema_for instead of
  extracting and relocating root definitions (crates/specs/ras-spec-components/src/components.rs).
- That file is embedded as source into generated code by both macros, and must compile in any edition.
- SpecGenerationError lives in ras-spec-components and is re-exported; the served documents cache it.
- The REST error body component is RestErrorBody; a 204 example is recorded under x-ras-examples.

Check that each point of the plan's "Round 1 review" and "Round 2 review" sections was done,
and say which were not.
Then look for a crash, data loss, race or security weakness the change brings in. In particular:
- error passthrough: crates/rpc/ras-jsonrpc-types/src/handler_error.rs, the generated dispatch in
  crates/rpc/ras-jsonrpc-macro/src/server/dispatch.rs, the bidirectional server, router and macro;
- the per-service OnceLock document caches in both static_hosting.rs files, and docs_require_auth;
- the params classifier and the {} fallback for absent params;
- generated code that could fail to compile in a consumer crate (hygiene, edition, feature gating, wasm).

Verdict first: ship, or blockers with file:line and a concrete fix. Mark each finding a blocker or a follow-up.
No praise, no summary.
