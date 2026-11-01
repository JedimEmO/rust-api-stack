**Changes needed.**

1. **Blocker — Dependency baseline: workspace Cargo.toml:59; Cargo.lock:3413.**

   The checkout does **not** use schemars alpha.20: `"1.0.0-alpha.20"` is a version requirement, and the lockfile resolves **1.2.1**. Shape assertions about alpha.20 would therefore test a different implementation. This matters for flattening and references; schemars changed both after alpha.20. [Schemars changelog](https://github.com/GREsau/schemars/blob/master/CHANGELOG.md).

   **Change:** make the plan target locked 1.2.1 and use `--locked` for validation. If alpha.20 compatibility is required, add a separate explicitly pinned fixture. Do not silently downgrade the workspace. Exact alpha.20 source was unavailable during this review, so the cases below should become executable fixtures rather than claims that its precise output has been verified.

2. **Blocker — “Generated OpenRPC”: the raw fallback still describes a failing call.**

   The statement that maps, arrays, enums and compositions are outside JSON-RPC is incorrect:

   - `Vec<u32>` accepts JSON-RPC `"params":[1,2]`.
   - `HashMap<String,u32>` accepts `"params":{"count":2}`.
   - An internally tagged enum accepts `"params":{"kind":"create","name":"Ada"}`.
   - A struct containing a flattened enum can accept an object while its schema contains composition keywords.

   A single descriptor named `params` still makes a conventional client wrap these as `{"params":{"count":2}}` or `[[1,2]]`. An extension does not repair that standard description.

   **Change:** distinguish *valid JSON-RPC params* from *representable by finite named descriptors*. Preserve raw calls for the former, explicitly document that their descriptors are not sufficient for standard client generation, and retain the complete schema extension. For a strictly interoperable document, fail **document generation** for unsupported shapes with the method and reason. Compile-time rejection based on a Rust type’s schema is not generally available to this macro.

3. **Blocker — “Generated OpenRPC”: define the object classifier and prove decoding, including empty params.**

   “Plain object” is too underspecified. These cases need different treatment:

   | Request | Descriptor-built call and consequence |
   |---|---|
   | `#[serde(rename_all="camelCase")] struct R { user_id:u32 }` | `{"userId":1}` decodes. Names must come from schema properties. |
   | `#[serde(deny_unknown_fields)] struct R { id:u32 }` | `{"id":1}` decodes; an invented `params` wrapper fails. `additionalProperties:false` must not disqualify it. |
   | Struct with `Option<String>` and `skip_serializing_if` | Omitting the field can decode; `None` must produce no example entry, rather than an invented null entry. |
   | Flattened ordinary struct | May expose merged properties; split the resulting schema, never Rust field names. |
   | Flattened map | Known fields plus arbitrary keys cannot be fully described by a finite property list. |
   | Flattened enum | Composition can impose dependencies between fields; independently valid descriptors do not capture them. |
   | `Envelope<User>` | Generics alone are no reason to fall back; classify its resolved schema. |
   | `struct R(Inner)` / transparent wrapper | Classify the inner wire shape, including references. |
   | Object reached through `$ref` | Resolve local references with cycle detection before classification; a property’s `$ref` is not itself grounds for raw fallback. |

   There is also an existing server mismatch at `server/dispatch.rs:93`: absent params becomes `null`. For `struct Empty {}` or an all-optional struct, a client seeing no required descriptors may omit params; `null` generally does not decode as that struct, whereas `{}` does.

   **Change:** specify a conservative classifier: resolve local root references; accept explicit object schemas with finite properties and no unrepresented composition or dynamic-key constraints. Preserve the whole schema. Define omitted-params behavior, either by accepting `{}` for eligible object requests server-side or requiring clients to send `{}`. Test descriptor-built requests against actual dispatch, including omitted params, `{}`, `[]`, and unit requests.

4. **Blocker — “Examples are typed”: type correctness does not establish wire correctness.**

   Concrete counterexample:

   ```rust
   #[serde(rename(serialize = "out", deserialize = "in"))]
   value: String
   ```

   A typed request example serializes as `{"out":"x"}` but the server expects `{"in":"x"}`. Custom serializers, `serde(into/from)`, skipped fields, and schemars overrides introduce similar gaps.

   Both generators currently use `schema_for!` for requests and responses. Schemars supports distinct serialization and deserialization contracts; its default contract is deserialization. A response schema therefore need not describe the serialized response. [Schemars contract introduction](https://github.com/GREsau/schemars/blob/master/CHANGELOG.md).

   **Change:** generate request schemas under the deserialization contract and response schemas under serialization, using separate component identities where necessary. Validate every serialized request example by deserializing it as the server request type, as well as checking its schema. Validate responses against serialization schemas. State that typed examples guarantee construction type correctness, with wire compatibility checked separately.

5. **Blocker — Feature gating: RPC `expand.rs:103`; REST `expand.rs:54`.**

   Neither macro has an `openrpc` or `openapi` Cargo feature. Those are macro options.

   - RPC emits document functions and schema checks at the expansion root whenever `openrpc` is configured, regardless of server/client features. That includes client-only and wasm consumers.
   - REST places document generation inside the server module.
   - Both proc-macro crates compile their generator implementation and schemars dependency regardless of the document option.

   Consequently, an RPC example calling `server_examples::request()` can break a wasm client build even when nobody invokes document generation. “Request types already serialize for clients” also misses server-only requests and client-only responses.

   **Change:** emit typed example bindings and expressions only within the enabled document-generation scope. Keep them absent when the document option is off. For RPC, add an explicit opt-in consumer gate for document generation, preserving existing root generation by default; server builds and export tools can enable it, wasm clients can disable it. Do not place example checks in shared permission or client metadata.

   Add compile fixtures for server-only, client-only, neither feature, native/wasm, both `feature_gated` modes, document options off, and a server-only request without `Serialize`.

6. **Blocker — “Handler errors at runtime”: source traversal grants unintended disclosure.**

   This handler has made no deliberate disclosure decision:

   ```rust
   upstream.call(...).await?;
   ```

   If the upstream client error exposes `JsonRpcError { code:1001, message, data }` through `source()`, the proposed traversal publishes its contents. A library error containing a JSON-RPC error has the same problem. Code 1001 passes the reserved-range filter; that filter controls protocol semantics, not confidentiality.

   Even a top-level bare `JsonRpcError` can arrive through an incidental `?` once it implements `Error`.

   **Change:** introduce a dedicated public-error marker, for example `PublicRpcError::new(code, message, data)`, and pass through only that marker returned directly in the handler’s error box. Keep `JsonRpcError` as a wire representation. Do not search arbitrary sources. Require an explicit conversion at the handler boundary when forwarding upstream errors. Replace the planned wrapped-error success tests with tests proving ordinary wrappers and upstream errors remain sanitized.

7. **Blocker — Error policy must cover every handler boundary, without leaking migration errors.**

   Relevant paths include:

   - HTTP canonical handler failure: `server/dispatch.rs:158`.
   - HTTP legacy handler failure: `server/dispatch.rs:234`.
   - Bidirectional generated server’s separate auth branches: `server.rs:84,116,175`.
   - Bidirectional router: `router.rs:88,173`.
   - Server-to-client request handlers: bidirectional macro `client.rs:139`.

   Filtering only in generated servers leaves a manually returned `ServerError::Rpc` able to bypass the policy if the router sends it unchanged. Applying one broad conversion to every error would instead expose response-migration or serialization failures.

   **Change:** centralize the code-policy check and sanitized conversion. Apply it at actual handler error boundaries and recheck `ServerError::Rpc` in both router conversion paths. Keep response migration and serialization errors internal. Retain request-migration failures as invalid params with a deliberately reviewed public message.

   Server-to-client generated handlers currently send handler `to_string()` under `-32000`; either include their sanitization in checkpoint 4 or explicitly exclude that direction from the claimed end-to-end guarantee. Test notifications still produce no response.

8. **Blocker — The example code contradicts the reserved-code rule; HTTP behavior needs an explicit contract.**

   The plan’s `duplicate_email` uses **-32010**, inside the prohibited range. Its documented error would become `-32603` at runtime.

   At HTTP `server/http.rs:193`, application code `1001`, permitted `-32602`, and sanitized `-32603` all get **HTTP 200**. The four framework auth/CSRF codes map to 401/403. Non-200 errors also take the warning-log branch at line 207. The generated HTTP client checks the JSON-RPC error body before falling back to HTTP status (`client.rs:143–178`).

   **Change:** replace `-32010` with `1001`; apply the same i32 bounds and reserved-code predicate to authored error literals and runtime public errors. Document HTTP 200 for application errors. Test both status and body through the generated client. I found no token-refresh behavior keyed on `-32003` in the inspected code; describe that as a possible downstream behavior, not an implemented client feature.

9. **Blocker — Checkpoint 1 needs a bounded dialect target.**

   Renaming `examples` is insufficient. Relevant incompatibilities include `const`, null schemas, type arrays, tuple `prefixItems`, numeric exclusive bounds, and map `patternProperties`. Current REST normalization only handles some nullable forms, and its unit component explicitly contains `"type":"null"`.

   OpenAPI 3.0 uses a restricted Schema Object; OpenRPC uses draft-07 schemas. Passing a meta-schema also does not establish correct reference resolution or matching example values. [OpenAPI 3.0 Schema Object](https://spec.openapis.org/oas/v3.0.3.html#schema-object), [OpenRPC Schema Object](https://spec.open-rpc.org/#schema-object).

   **Change checkpoint 1 to:**

   - Vendor exact schemas and their dependencies; disable network retrieval.
   - Validate the existing fixture documents without broadening their type inventory.
   - Use schemars’ dialect settings for OpenAPI 3.0 and draft-07 where supported by the chosen locked version.
   - List and fix only incompatibilities demonstrated by those fixtures.
   - Separately assert all local references resolve and component keys are legal.
   - Defer general conversion of arbitrary custom schemas, recursive composition, and unsupported tuples/maps to a named follow-up.

   Do not recursively rewrite arbitrary JSON objects: example payloads can legitimately contain keys named `examples`, `$ref`, or `type`.

10. **Blocker — “Generated OpenAPI”: the advertised errors and statuses exceed the proposed implementation.**

   `ErrorBody` matches handler `RestError` output at `server/handlers.rs:75`. It does not automatically describe Axum path/query extraction failures or every framework rejection. An invalid numeric path parameter can return a different body before the handler runs.

   Also, `status:201` needs a corresponding success response entry; adding only statuses from error examples misses it. `204` cannot carry the proposed JSON response example.

   **Change:** describe `ErrorBody` specifically for the paths that emit it, or normalize framework rejection bodies in a separately scoped runtime change. Add response entries for success-example statuses, restrict them to supported 2xx responses, and reject body examples for bodyless statuses. Require every non-optional query parameter; the current syntax section only explicitly requires path parameters.

11. **Blocker — Runtime example failure needs a fallible generation API.**

   A map with tuple keys or a custom serializer returning `Err` can make a served document panic. Skipping one part can leave a pairing incomplete; silently skipping the whole example hides broken documentation.

   **Change:** add `try_generate_*() -> Result<Value, SpecGenerationError>` with service, operation, example, part, and cause. Serialize each complete example atomically. Export-to-file propagates failure; serving returns a generic 500 and logs context without logging example values. Generate/cache documents at startup where possible.

   Preserve the existing infallible function as a compatibility wrapper if its signature must remain stable. CI should exercise the fallible function and fail on any omitted or invalid example. Require example expressions to be deterministic and side-effect free; repeated GETs must not repeatedly execute arbitrary example construction.

12. **Follow-up — Compatibility and release inventory is incomplete.**

   The proposed minor bumps for the three changed error enums and bidirectional macro are appropriate. Additional decisions affect the table:

   - A dedicated public-error marker is additive in `ras-jsonrpc-types`: patch remains reasonable.
   - Replacing an existing generator’s return type with `Result` would be a generated public API break, including REST; adding a `try_` function avoids that.
   - Changing RPC document gating must preserve existing export-function availability by default or be covered by its minor bump.
   - Generated client method signatures should remain unchanged; their error payload/variant contracts change. Add signature compatibility checks.
   - Update dependency requirements on every bumped crate, plus Cargo.lock—not just package versions.
   - Update `examples/basic-jsonrpc`, `oauth2-demo`, REST wasm, and bidirectional chat builds/tests. Do not bump unpublished examples.
   - Keep permission-manifest operation IDs, wire names, auth groups, and legacy entries unchanged and test that explicitly. No manifest crate bump is required solely because the macro AST grows.
   - Explorer extensions and configuration compatibility deserve regression tests; no configuration break is currently specified.

13. **Follow-up — Planned validation and explorer tests miss semantic failures.**

   A document can validate while describing an unusable call. Add targeted tests for:

   - Requests reconstructed from descriptors and accepted by real dispatch.
   - Serialization/deserialization renames, flattened maps/enums, reference cycles, empty objects, generic requests, and newtypes.
   - Allowed/reserved code boundaries, positive codes, upstream nested errors, and both router registration APIs.
   - Legacy handler errors versus request/response migration errors.
   - HTTP and WebSocket clients preserving code, message, and data, including native/wasm compilation.
   - Error-only examples, omitted optional query values, false/zero/empty-string values, and switching examples clearing previously filled fields.
   - REST examples spread across different statuses and examples without bodies.
   - Example names containing invalid OpenRPC component-key characters: Rust identifiers alone are not sufficient validation.
   - DOM behavior and the actual outgoing request; HTML string-presence assertions cannot prove editors were populated correctly.

   Define same-code error deduplication explicitly: keep method error codes unique, preserve individual example messages/data, and reject collisions with framework error definitions rather than silently replacing them.

For your open questions:

1. **Serialization failure:** use fallible, atomic generation with contextual errors; fail CI/export and return a sanitized HTTP 500. Avoid skipping examples or panicking in the serving path.
2. **Built-in handler codes:** keep the initial allowlist at application codes plus `-32602`. Use an application code for handler-owned auth checks initially. If framework auth codes are later needed, expose named constructors with documented HTTP consequences, rather than permitting arbitrary reserved codes.
3. **Example placement:** keep operation pairing metadata in the macro. Allow function calls as value expressions—`params: examples::basic_request()` already fits the design and keeps large values elsewhere. Type-level attributes cannot express operation-specific pairing. No additional provider syntax is needed initially.
4. **Raw fallback:** retain it for compatibility, but label its standard-tooling limitation accurately. Do not reject all non-object requests at compile time. Offer strict document generation that rejects shapes the standard descriptors cannot faithfully represent, while keeping raw transport behavior intact.
