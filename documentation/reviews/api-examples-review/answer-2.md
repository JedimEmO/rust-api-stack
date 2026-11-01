**The blockers still open.**

1. **Blocker — Plan “Decisions”, “Fallible generation”, and Round 1 ruling 4: successful decoding is insufficient.**

   This request example can serialize, deserialize successfully, and validate against the deserialization schema while losing its authored value:

   ```rust
   struct Request {
       #[serde(default, rename(serialize = "out", deserialize = "in"))]
       value: String,
   }
   ```

   `Request { value: "Ada".into() }` serializes as `{"out":"Ada"}`. Deserialization ignores `out` and defaults `value` to `""`. The schema permits unknown properties and does not require the defaulted field. The generated named example then contains `out`, for which there is no descriptor.

   **Change:** define the request check precisely: serialize, deserialize into the declared request type, serialize the decoded value again, and require JSON equality. Report an incompatible example if it changes. For named examples, also require every serialized key to have a descriptor. This adds no `PartialEq` requirement.

2. **Blocker — Plan “Follow-ups”: deferring response serialization schemas leaves finding 4 open.**

   For a response with:

   ```rust
   #[serde(rename(serialize = "out", deserialize = "in"))]
   value: String
   ```

   the server emits `out`, while the document describes `in`. Rejecting its example or asking users to change the type does not make the generated response description correct. This is ordinary supported serde behavior, not general custom-schema conversion.

   **Change:** use the serialization contract for response schemas now. Key schema collection by **type and contract**, so a type used for both request and response can have distinct components. Give those components deterministic, collision-checked names. Both macros already take minor bumps in this revision.

3. **Blocker — Plan “Where examples compile”: the simplification omits a new `Deserialize` requirement.**

   Keeping document functions at their existing scope is workable. However, runtime round-tripping introduces a bound the section does not mention.

   A client-only request currently needs `Serialize` and, with OpenRPC enabled, `JsonSchema`. It need not implement `Deserialize`. Adding an example under this revision makes that build fail because the root-level document function now deserializes it—even if that function is never called.

   **Change:** explicitly require both serialization and owned deserialization for request types that have authored examples. Document that requirement for RPC client-only/wasm builds, and add compile fixtures showing:

   - A request without `Deserialize` still compiles without authored examples.
   - Adding an example produces a diagnostic at that example.
   - No new example bounds or expressions appear with the document option off.

   No new gate is necessary if this additional opt-in requirement is accepted.

4. **Blocker — Plan “Parameters”: the classifier still admits requests with dynamic keys or cross-field constraints.**

   It rejects schema-valued `additionalProperties` but allows `true`. A flattened `HashMap<String, serde_json::Value>` can therefore escape the intended raw fallback. Arbitrary entries are part of the request, yet finite named descriptors cannot describe them.

   The exclusion list also misses draft-07 `dependencies`. For example:

   ```json
   {
     "type": "object",
     "properties": {
       "a": {"type": "string"},
       "b": {"type": "string"}
     },
     "dependencies": {"a": ["b"]}
   }
   ```

   Both descriptors appear optional; sending only `a` satisfies the descriptors but violates the complete schema.

   **Change:** reject explicit `additionalProperties:true`, `dependencies`, and other unrepresented object-wide constraints such as `minProperties`/`maxProperties`. Treat omitted `additionalProperties` separately so ordinary structs remain eligible. Add a flattened `HashMap<String, Value>` fixture, not just a typed-value map. For unsupported constraints, choose raw rather than attempting to simplify them.

5. **Blocker — Plan “Parameters”: “absent” currently includes explicit JSON null.**

   `JsonRpcRequest.params` is `Option<Value>` (`ras-jsonrpc-types/src/lib.rs:18`). Serde decodes both an omitted field and `"params":null` as `None`.

   After the proposed fallback, this request starts reaching an empty-struct handler:

   ```json
   {"jsonrpc":"2.0","id":1,"method":"empty","params":null}
   ```

   The plan promises `{}` fallback only for absent params, but the existing representation cannot enforce that distinction.

   **Change:** preserve present-null during request decoding—for example, a field deserializer with `#[serde(default)]` that wraps a present `Value` in `Some`. Apply fallback only to genuinely absent params. Keep present values on the existing decoding path.

   Test absent params, explicit null, `{}`, and `[]` separately through canonical and legacy HTTP dispatch. Keep fallback independent of document generation so enabling OpenRPC cannot change server acceptance.

6. **Blocker — Checkpoint 1: dialect selection requires explicit extraction and relocation changes.**

   Switching only the generator settings breaks the current normalization:

   - RPC extraction handles `$defs` and `#/$defs/` only (`openrpc/schema.rs`, `generate_normalization`). Draft-07 emits `definitions` instead.
   - REST extraction handles `definitions` and `$defs` (`openapi/schema.rs:138`). With `openapi3()`, a generated root instead contains its definitions under **that root’s** `components.schemas`. Those are not automatically the document’s top-level components.
   - Recursive root references can be `"$ref":"#"`. After embedding the schema in a document, `#` points at the API document, not the original type schema.

   These behaviors are explicit in [schemars 1.2.1’s generator source](https://docs.rs/schemars/1.2.1/src/schemars/generate.rs.html).

   **Change:** spell out the assembly procedure:

   - RPC: extract root `definitions`, then relocate its references into document components.
   - REST: extract root `components.schemas`, merge them into document components, and remove the nested container and root `$schema`.
   - Both: rebase root-relative references, including `#`, to the assigned component location.
   - Maintain a mapping from generated names to final component keys; rewrite references through it. Fail on conflicting definitions instead of silently overwriting them.
   - Remove the old nullable conversion passes; retain only schema-aware extraction and relocation.

   Add nested-type, recursive-root, generic-root, and conflicting-name fixtures. A test that merely checks “every reference resolves” misses `#`: it resolves successfully to the **wrong object**. Assert expected reference targets too.

7. **Blocker — Checkpoint 1 and Explorer: `allOf` wrappers introduce a rendering regression.**

   Both selected settings move `$ref` siblings into `allOf`. A documented field referencing another struct can become:

   ```json
   {
     "description": "Delivery address",
     "allOf": [{"$ref":"#/components/schemas/Address"}]
   }
   ```

   [Schemars’ transformation source](https://docs.rs/schemars/1.2.1/src/schemars/transform.rs.html) confirms this rewrite.

   The explorer’s `schema-model.js` resolves only direct `$ref`s; `schema-render.js` does not handle `allOf`. This field can therefore display as an empty object and get `{}` as its guessed value despite `Address` having required fields. The new classifier also classifies an annotation-wrapped root reference as raw without ever reaching its object schema.

   **Change:** support the narrow wrapper produced here: a single-reference `allOf` with annotation-only siblings. Resolve it with cycle detection while preserving description/title. Use that interpretation in explorer field rendering and value guessing, and optionally in classification. Keep general intersections raw.

   Add a fixture with a documented referenced field and verify its fields and outgoing guessed request. This belongs in checkpoint 1/2 because the dialect change introduces it before authored explorer examples.

8. **Blocker — Checkpoints 5/6: example validation needs an explicit OpenAPI-aware interpretation.**

   With `openapi3()`, an optional string schema can be:

   ```json
   {"type":"string","nullable":true}
   ```

   An ordinary JSON Schema validator rejects `null` against that schema: `nullable` does not change JSON Schema’s `type` semantics. The revised instruction to validate every example with `jsonschema` therefore rejects legitimate examples.

   **Change:** separate document validation from example-value validation. For published OpenAPI schemas, use an OpenAPI-aware evaluator or a test-only, schema-aware translation of the supported OpenAPI subset into draft-07, including `nullable` and reference context. Reject unsupported translation cases explicitly. Include `None` for primitive and referenced object options; testing only `Some(...)` misses this failure.

9. **Blocker — Plan “Fallible generation”: specify cache identity, failure caching, and authorization order.**

   “Kept in a `OnceLock`” does not establish these properties. A shared cache could serve service A’s document for service B; caching an authorized response could bypass `docs_require_auth` on subsequent requests; caching only success would repeatedly execute failing example constructors.

   Current RPC authorization wraps the explorer routes (`server/http.rs:85` onward). REST authorizes before invoking `openapi_json_handler` (`static_hosting.rs:160`). Preserve those boundaries.

   **Change:** require a function-local static in each generated service’s document-serving handler:

   ```rust
   OnceLock<Result<Value, CachedSpecGenerationError>>
   ```

   Initialize it with `try_generate_*`, cache failure as well as success, and cache **only generation output**. Perform authorization on every request before consulting the cache. Do not cache headers, caller information, route HTML, or an authorization decision. Keep cached error storage `Send + Sync`, and namespace any generated error type/helper per service or put it in a shared support crate.

   Add tests for two services with distinct documents, concurrent first requests, cached failure, and authenticated-then-unauthenticated requests against a warm cache. Also verify an unauthorized cold request does not initialize it.

10. **Follow-up — Plan “Handler errors at runtime” and checkpoint 3: distinguish conversion from code validation.**

   Top-level `PublicRpcError` downcasting closes the source-traversal disclosure. The remaining specification issue is that a function accepting a boxed handler error cannot also directly revalidate `ServerError::Rpc(JsonRpcError)`: the latter contains a wire value, which deliberately does not implement `Error`.

   **Change:** define one shared code predicate, used by macro literal checks and runtime validation, plus explicit conversion entry points:

   - Boxed handler error → top-level marker check → validated wire error.
   - `ServerError::Rpc` payload → code validation directly.
   - Other `ServerError` variants → existing sanitized handling.

   Do not run the application-code rule over every outgoing response; framework auth responses legitimately use the reserved codes.

   Checkpoint 3 also asks for “a `JsonRpcError` returned as an error.” That cannot compile under the revised design. Replace it with a compile-fail case for direct boxing, and a runtime test of an ordinary error wrapper containing a `JsonRpcError`, which must remain sanitized.
