**Blockers.** All locations below refer to `0297c76`, reviewed against `15404c3`.

1. **Blocker — Rust 2018 consumers cannot compile the embedded source.**  
   `crates/specs/ras-spec-components/src/components.rs:173`

   `<&mut Schema>::try_from(value)` requires `TryFrom` in scope. That trait entered the prelude in edition 2021; the embedded module does not import it. Both macros emit this code whenever document generation is enabled, even without examples.

   The edition-2021 test crate therefore misses the compatibility failure. A minimal edition-2018 reproduction produces E0599.

   **Fix:** import `::std::convert::TryFrom` in the embedded source, or use a fully qualified trait call. Add actual edition-2018 consumer checks for both macros; compiling the source as edition 2021 does not establish compatibility with earlier editions.

2. **Blocker — Generated locals capture authored example expressions.**  
   `crates/rpc/ras-jsonrpc-macro/src/openrpc/methods.rs:334`  
   `crates/rest/ras-rest-macro/src/openapi/operations.rs`, `example_tokens`

   A valid helper such as:

   ```rust
   fn context() -> String {
       "pong".into()
   }
   ```

   cannot be used as `result: context()`. The generated example block first declares its own `context` closure taking two arguments, so the authored expression resolves to that closure and fails compilation. The surrounding `generators` binding creates another possible collision.

   **Fix:** give generated local bindings and their references hygienic `Span::mixed_site()` identifiers. Keep authored expressions’ original spans. Add consumer tests using helpers named `context` and `generators`; parser tests cannot detect this failure.

3. **Blocker — Selecting an empty-string query example changes the request.**  
   `crates/specs/ras-api-explorer-assets/src/assets/examples.js:105`  
   `crates/specs/ras-api-explorer-assets/src/assets/requests.js:36`

   The committed REST example `empty_query` supplies required `q: String` as `""`. Applying it fills the input with an empty string, but request construction includes only truthy input values. The outgoing URL is `/search`, rather than `/search?q=`; the server can reject it for missing `q`.

   The test at `crates/rest/ras-rest-macro/tests/examples.rs:258` verifies the document preserves `""`, but the Playwright test does not send that case. I reproduced the omission using the pinned JavaScript.

   **Fix:** track whether a query value is present separately from its text. Applying an example must distinguish an omitted parameter from a present empty string; URL construction must preserve the latter. Test selecting, sending, and then clearing an empty-string example.

4. **Blocker — String request-body examples become invalid JSON in the editor.**  
   `crates/specs/ras-api-explorer-assets/src/assets/examples.js:109`

   For a REST endpoint accepting `String`, an authored `body: "hello".to_owned()` becomes the JSON string value `"hello"`. `applyExample` passes it through `jsonPretty`, which returns string values without quotes. The editor receives `hello`, and `buildRequest` throws while calling `JSON.parse`.

   An empty-string body becomes an empty editor and is omitted entirely. I reproduced the nonempty case from the pinned JavaScript.

   **Fix:** use `JSON.stringify(value, null, 2)` when populating JSON request editors, including explicit null and string values. Retain `jsonPretty` only where displaying unquoted text is intended. Add outgoing-request tests for `"hello"` and `""`.

5. **Blocker — Named examples lose a field named `__proto__`.**  
   `crates/specs/ras-api-explorer-assets/src/assets/examples.js:15`

   This request is valid:

   ```rust
   struct Request {
       #[serde(rename = "__proto__")]
       value: serde_json::Value,
   }
   ```

   Its descriptor and pairing correctly contain `__proto__`. However, reconstruction uses `{}` followed by `value[param.name] = param.value`. Assigning that property invokes the inherited prototype setter rather than creating the required own property. Serializing the reconstructed params loses the field; my reproduction produced `{}`.

   **Fix:** reconstruct dictionaries using `Object.create(null)` or `Object.fromEntries`. Apply the same treatment to the new REST path/query dictionaries. Test the actual outgoing request for a renamed `__proto__` field.

6. **Follow-up — Component-key validation promised by the plan is absent.**  
   `crates/specs/ras-spec-components/src/components.rs:94`

   The merge checks `.Serialize` collisions, but never checks the component-key pattern. A type renamed through schemars to a name containing spaces or `/` can make `try_generate_*` return—and the serving cache retain—a document that fails the API meta-schema.

   This omission is not recorded in “Build notes”; changing assembly strategy does not remove the naming constraint.

   **Fix:** validate final component keys before returning `Components`, reporting an error for an illegal key. Add a renamed-type fixture. Automatic name conversion is unnecessary.

7. **Follow-up — `RestErrorBody` remains unreferenced for ordinary handler errors without error examples.**  
   `crates/rest/ras-rest-macro/src/openapi/operations.rs:218`  
   `crates/specs/ras-spec-components/src/components.rs:604`

   The default 500 response still has only a description. Its handler-error content schema is added only when an authored error example uses that status. An endpoint returning `RestError` without error examples therefore retains the missing error-body description this change intended to fix.

   **Fix:** attach the handler-error schema to the known handler-error response statuses independently of examples, with the same description distinguishing framework rejection bodies. Keep example insertion separate from response-schema construction.

The review-ruling audit is:

| Ruling | Result in the commits |
|---|---|
| Round 1: **1** version, **2** raw fallback, **3** classifier/absent params | Implemented. |
| Round 1: **4** wire correctness | Implemented through equality round-tripping and separate response contracts, superseding the original deferral. |
| Round 1: **5** feature gating | Implementation/documentation follow the simplified ruling; promised consumer compile coverage is incomplete. |
| Round 1: **6** disclosure, **7** error boundaries, **8** code policy | Implemented within the explicitly retained scope. |
| Round 1: **9** bounded validation | Implemented, subject to the missing key check above. |
| Round 1: **10** error bodies/statuses | Success statuses and 204 handling implemented; handler error schemas remain conditional on examples. |
| Round 1: **11** fallible generation/cache | Implemented. |
| Round 1: **12** release inventory | Versions, dependency requirements, lockfile, documentation, and examples were updated. |
| Round 1: **13** tests | Partial: substantial runtime/browser coverage, but not the promised consumer compilation matrix; the explorer cases above are missing. |
| Round 2: **1**, **2** equality/response contracts | Implemented. |
| Round 2: **3** new deserialization bound | Documented; the promised request-without-`Deserialize` compile fixture was not committed, as Build notes acknowledge. |
| Round 2: **4**, **5** classifier/present null | Implemented. The fallback applies only to absent params, independently of OpenRPC. |
| Round 2: **6** assembly | Replaced by the documented generator-based assembly; component-key validation was omitted. |
| Round 2: **7**, **8** wrappers/nullable validation | Implemented for the stated supported subset. |
| Round 2: **9** cache identity/auth | Implemented: per-service function-local caches retain generation results, and existing authorization remains outside the cache. Cold/warm, failure, concurrency, and service-separation tests are committed. |
| Round 2: **10** error conversion | Implemented: one predicate, separate handler/wire entry points, no source traversal. |

Notification suppression was **not implemented**; Build notes explicitly withdraw it as a pre-existing issue. Server-to-client error sanitization likewise remains the declared follow-up.

I reproduced the JavaScript failures and the edition-2018 trait-scope failure. I did not run the full Cargo or Playwright suites.
