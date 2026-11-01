Plan, revision 3, 2026-11-01. Based on `15404c3`.
Revises [revision 1](api-examples-plan.md) and [revision 2](api-examples-plan-2.md)
after two review rounds
([brief 1](api-examples-review/brief-1.md), [answer 1](api-examples-review/answer-1.md),
[brief 2](api-examples-review/brief-2.md), [answer 2](api-examples-review/answer-2.md));
the rulings are in the last two sections.
This is the agreed plan.

Adds authored request/response examples and error examples to the OpenRPC and OpenAPI documents
the service macros generate, and lets JSON-RPC handlers return errors a client can act on.
Two fixes come with it, because the examples would expose them:
the OpenRPC description of `params` and the missing error schemas in OpenAPI.
All schema behaviour below refers to schemars 1.2.1, the version `Cargo.lock` resolves.

## The problem

A reader of a generated document sees types, but never a call.
Neither macro accepts examples.
The OpenRPC generator builds a placeholder example from each schema
(`"example_string"`, `42`) and then discards it;
commit `3869734` removed it from the output because "some validators" rejected it.
Placeholder values fail schemas with enums, formats or tagged unions,
which is the likely cause.

The OpenRPC document describes the wrong call shape.
For `create_user(CreateUserRequest)` it lists one parameter named `params`
with the default `paramStructure`.
A conforming tool then sends `{"params": {"name": ...}}` by name or `[{"name": ...}]` by position.
The server (`server/dispatch.rs`) decodes `params` itself as the request type and rejects both.
Only our explorer works, because it reads `params[0].schema` as the whole request.

JSON-RPC handlers cannot report their own errors.
Every `Err` becomes `-32603 "Internal error"` with no detail,
in `ras-jsonrpc-macro` dispatch and in the bidirectional macro and router alike.
So the only errors a JSON-RPC client can ever see are the built-in ones,
and an error example could document nothing but auth and decoding failures.

REST handlers can already choose a status and message through `RestError`,
which goes out as `{"error": message}`.
The OpenAPI document does not say so; the 4xx and 5xx responses have no content schema.
schemars also emits JSON Schema 2020-12 constructs into OpenAPI 3.0 component schemas
(`examples`, `const`, type arrays, `prefixItems`), which OpenAPI 3.0 does not allow.

Nothing checks the generated documents against the OpenRPC or OpenAPI schemas,
so none of this was caught.

## Decisions

Examples are typed Rust values written in the macro.
The generated code assigns each value to the declared request or response type,
so an example that no longer fits its type fails to compile.
Type correctness is not wire correctness, though;
serde attributes can make the serialized form differ from what the server decodes.
Document generation therefore round-trips every request example:
serialize it, deserialize it as the request type, serialize again,
and require the two JSON values to be equal.
A `#[serde(default, rename(serialize = "out", deserialize = "in"))]` field
decodes without error but loses its value, and the equality check catches that.
Tests validate every example against its schema.

Request schemas describe what the server decodes,
and response schemas describe what it sends.
Generation uses schemars' deserialization contract for requests
and its serialization contract for responses.

The OpenRPC document describes JSON-RPC parameters by name
when the request schema is an object that a finite list of descriptors can describe.
Other request types keep the single raw descriptor, labelled as such.

A handler opts into a wire error by returning a new `PublicRpcError` directly.
Nothing else reaches the client: not a `JsonRpcError`, and not an error found inside another error.
The handler signature keeps `Box<dyn Error + Send + Sync>`,
so existing handlers compile and behave as before.

Document generation becomes fallible, because examples can fail to serialize.
Existing generator functions keep their signatures.

Placeholder examples leave the documents.
The explorer keeps guessing values from schemas when an operation has no examples.

## Macro syntax

Examples sit in the existing options block after the response type.
Each example has an identifier, unique within the method,
and an optional doc comment that becomes its summary and description,
parsed the same way as method docs.

```rust
/// Create a user account.
WITH_PERMISSIONS(["admin"]) create_user(CreateUserRequest) -> User {
    examples: [
        /// A user with only the required fields.
        basic {
            params: CreateUserRequest { name: "Ada".into(), email: "ada@example.com".into() },
            result: User { id: 1, name: "Ada".into() },
        },
        /// The e-mail address is already registered.
        duplicate_email {
            params: CreateUserRequest { name: "Ada".into(), email: "taken@example.com".into() },
            error: { code: 1001, message: "Email already registered", data: EmailTaken { field: "email".into() } },
        },
    ],
}
```

An example has exactly one of `result` and `error`.
`params` is required unless the request type is `()`.
`code` must be an integer literal and `message` a string literal, so the macro can check them.
`code` must pass the same rule as handler errors at runtime (see below),
so a documented error is one the server can actually send.
`data` is any expression whose type implements `Serialize`.
Values may be function calls, such as `params: examples::basic_request()`,
which keeps large values out of the macro.

REST examples name each part of the request:

```rust
/// Rename an item.
POST WITH_PERMISSIONS(["items:write"]) items/{id: u32}/rename ? notify: Option<bool> (RenameItem) -> RenamedItem {
    examples: [
        /// Rename and notify watchers.
        notify_watchers {
            path: { id: 42 },
            query: { notify: Some(true) },
            body: RenameItem { name: "Spare parts".into() },
            response: RenamedItem { id: 42, name: "Spare parts".into() },
        },
        /// The item does not exist.
        missing_item {
            path: { id: 9999 },
            body: RenameItem { name: "Spare parts".into() },
            error: { status: 404, message: "Item not found" },
        },
    ],
}
```

Every path parameter and every non-`Option` query parameter must be given;
`Option` query parameters may be left out.
`body` is required exactly when the endpoint declares a request type.
A success example may set `status` to a 2xx literal, defaulting to 200;
`204` takes no `response`.
An error `status` must be a 4xx or 5xx literal.

The macro reports a missing or unknown field, a duplicate example name,
a `result` together with an `error`, and a code or status outside its rule,
at the offending tokens.
Values are emitted with `quote_spanned!` into typed bindings,
so a type mismatch points at the example, not at the macro call.

Example expressions must be deterministic and free of side effects;
the document may be generated more than once.

## Where examples compile

Neither macro has an `openrpc` or `openapi` Cargo feature; both are macro options.
`ras-jsonrpc-macro` emits the document functions at the root of the expansion,
outside the server and client modules, so they compile in client-only and wasm builds too.
`ras-rest-macro` emits them inside the server module.

Examples compile exactly where the document functions compile, and nowhere else.
They are absent when the document option is off.
For a JSON-RPC service that means example values must be available in client builds as well,
and so must the bounds examples add:
`Serialize` on every example type, and `DeserializeOwned` on a request type with examples,
for the round trip.
A client-only request type without `Deserialize` keeps compiling until an example is added;
the error then points at the example.
Moving the JSON-RPC document functions behind a new gate would change
where an existing public function exists, which this change does not need;
the requirement is documented instead.

## Generated OpenRPC

### Parameters

The classifier resolves local `$ref`s at the root of the request schema, with cycle detection,
including a reference wrapper (described under schema assembly).
A request schema is *named* when the result has `type: "object"`, a finite `properties` map,
and no other object-wide constraint the descriptors cannot carry:
no top-level `oneOf`, `anyOf`, `allOf`, `not`, `if`, `dependencies`,
`patternProperties`, `propertyNames`, `minProperties` or `maxProperties`,
and `additionalProperties` either absent or `false`.
`false` comes from `deny_unknown_fields`.
`true` or a schema means arbitrary keys, as from a flattened `HashMap<String, Value>`,
and makes the request raw.
Anything the classifier does not recognise makes the request raw, never simplified.
Property names come from the schema, so `rename_all` and `rename` are respected;
a property whose own schema is a `$ref` is fine.
Generic types and newtype or transparent wrappers are classified by their resolved schema,
not by their Rust shape.

A named request gets `"paramStructure": "by-name"` and one content descriptor per property.
`required` comes from the schema's `required` list,
and the description from the property, which carries the field's doc comment.

Every other request is *raw*.
It keeps the single `params` descriptor and is marked `x-ras-params-raw: true`.
Many raw requests are valid JSON-RPC: `Vec<u32>` takes `"params": [1, 2]`,
a map or an internally tagged enum takes an object.
But a standard tool reads the single descriptor as a wrapper and sends `{"params": ...}`,
so the document states that raw methods cannot be called from the descriptors alone.
Only a primitive request, such as `u32`, is outside JSON-RPC 2.0 itself.

Both kinds carry the whole request schema in `x-ras-params-schema` as a `$ref`.
A `()` request has no parameters.

A named request with no required properties can be called with `params` left out.
Today the server decodes a missing `params` as `null`, which fails for a struct.
Dispatch will try `{}` when `params` is absent and `null` does not decode,
so the call the descriptors permit is accepted.

`JsonRpcRequest.params` is an `Option<Value>`,
and serde turns both a missing field and `"params": null` into `None`.
Its deserializer changes so that a present `null` becomes `Some(Value::Null)`;
the field type stays, and serialization is unchanged.
Only a missing field gets the `{}` fallback, and explicit `null` decodes exactly as today.
The fallback lives in dispatch, independent of document generation,
so turning `openrpc` on or off never changes which calls the server accepts.

### Examples and errors

Success examples become standard Example Pairing objects.
`params` holds one Example per key of the serialized request, for named methods,
or one `params` Example for raw ones; a skipped `None` field has no entry.
Every key must match a descriptor; a key without one fails generation.
`result` holds the serialized response.

An Example Pairing has no place for an error.
Error examples go into `x-ras-error-examples` on the method,
each with `name`, `summary`, `description`, `params` and an `error` object of `code`, `message` and `data`.

Each method also gets the standard `errors` list.
It references the built-in errors in `components.errors` that the method can produce,
from `-32602` for every method that takes parameters
to the authentication and permission errors for `WITH_PERMISSIONS`.
Application codes from error examples are added once per code, with the first example's message;
each example keeps its own message and data in `x-ras-error-examples`.
An error example with `-32602` references the built-in `InvalidParams` entry.

## Schema assembly

Each macro generates one schemars root schema per type and contract,
then moves the definitions inside it into the document's components.
Today that is done by passes that walk whole values;
the REST pass removes a `definitions` key from any nested object,
and the JSON-RPC pass knows only `$defs`.
Both are replaced by one procedure, run per root schema:

1. Generate with `SchemaSettings::draft07()` for OpenRPC and `SchemaSettings::openapi3()` for OpenAPI,
   with the contract the type is used under.
2. Take the definitions out of the root's own container:
   `definitions` for draft-07, the root's nested `components.schemas` for `openapi3()`.
   Remove the container and the root's `$schema`.
3. Assign every definition and the root a final component key.
   A type used under both contracts keeps its name when both schemas are equal;
   otherwise the response schema gets the key `<Name>.Serialize`.
   Keys are deterministic, checked against the component key pattern,
   and a key that would hold two different schemas fails generation.
4. Rewrite references through that mapping, visiting schema positions only.
   This includes root-relative references such as `"$ref": "#"` from a recursive type,
   which point at the API document once the schema is embedded.
   Example values are never rewritten.

The hand-written nullable conversion passes go away; `openapi3()` emits `nullable` itself.

Both settings move the siblings of a `$ref` into an `allOf`.
A documented field that refers to another struct becomes
`{ "description": "Delivery address", "allOf": [{ "$ref": "#/components/schemas/Address" }] }`.
This *reference wrapper*, an `allOf` with exactly one `$ref` and only annotation siblings,
is read as that reference by the classifier and by the explorer,
keeping the description.
Every other `allOf` stays a real intersection: raw for the classifier,
and the explorer's current handling.

## Generated OpenAPI

OpenAPI 3.0 has no request/response pairs either.
An example appears under the same key in each place it touches:
`parameters[].examples`, `requestBody.content."application/json".examples`
and `responses.<status>.content."application/json".examples`.
Swagger UI and Redoc show the parts side by side under that name,
and the explorer pairs them by key.

Every status an example uses gets a response entry, success and error alike.
A new `ErrorBody` component schema (`{ "error": string }`)
describes the error responses that handlers produce through `RestError`.
Framework rejections that happen before the handler,
such as a path parameter that fails to parse, have other bodies;
the 400 response says so in its description and carries no `ErrorBody`.

## Fallible generation

An example can fail at document generation time:
a map with non-string keys, a custom serializer returning `Err`,
or a request example whose serialized form does not decode back into the request type.

Each macro gains `try_generate_<service>_openrpc()` (or `_openapi()`),
returning `Result<serde_json::Value, SpecGenerationError>`.
The error names the service, operation, example, part and cause, and never the example's value.
Each example is serialized as a unit; a failing example fails the document, never only itself.

The existing `generate_<service>_openrpc()` keeps its signature and panics with that error.
The `_to_file` functions propagate it as `std::io::Error`.

Today the served `openrpc.json` and `openapi.json` are regenerated on every request.
Each generated document handler gets a function-local
`static DOC: OnceLock<Result<Value, CachedSpecError>>`, initialised with `try_generate_*`.
Being function-local in a per-service handler, it cannot serve one service's document for another.
It caches failure as well as success, so a failing example runs once;
a failure answers with a generic 500 and logs the context.
It caches generation output only: no headers, caller or authorization decision.
Authorization stays where it is, wrapping the JSON-RPC explorer routes and running
before the REST handler, on every request,
so an unauthorised request never reaches the cache, cold or warm.
`CachedSpecError` is `Send + Sync` and lives in `ras-jsonrpc-core` and `ras-rest-core`,
which the generated code already depends on, not in generated code.

## Handler errors at runtime

`ras-jsonrpc-types` gains `PublicRpcError { code, message, data }`,
which implements `Display` and `std::error::Error`.
A handler returns it with `Err(PublicRpcError::new(1001, "Email already registered").with_data(...).into())`.
`JsonRpcError` stays a wire type and does not implement `Error`.

The policy sits in `ras-jsonrpc-types` as one code predicate and two entry points.
The predicate is shared with the macros, which apply it to example literals.
The first entry point takes a handler's boxed error,
downcasts it to `PublicRpcError` and nothing else, and does not walk `source()`.
The second takes the `JsonRpcError` inside a `ServerError::Rpc` and checks its code.
Neither runs over outgoing responses in general,
because framework auth responses legitimately use reserved codes.
An upstream error forwarded with `?`, even one that contains a JSON-RPC error,
stays the sanitized `-32603`.
Forwarding it means converting it to a `PublicRpcError` in the handler, deliberately.

A `PublicRpcError` code must be an `i32` outside the reserved range `-32768..=-32000`, or `-32602`.
Any other reserved code is logged and replaced by `-32603`.
The reserved codes carry protocol meaning, and the HTTP layer maps
`-32001`, `-32002`, `-32003` and `-32004` to 401 and 403;
a handler must not produce those by accident.
Application codes and `-32602` are sent with HTTP 200, as other JSON-RPC errors are today.

The boundaries are:

- the canonical handler in HTTP dispatch (`server/dispatch.rs`, the `Err(e)` after the handler call);
- the legacy-version handler in the same file;
- the three auth branches of the bidirectional generated server (`ras-jsonrpc-bidirectional-macro/src/server.rs`),
  which map to a new `ServerError::Rpc(JsonRpcError)`;
- both conversion paths in the bidirectional router (`router.rs`), which apply the code rule again,
  so a hand-written `ServerError::Rpc` cannot bypass it.

Failures that are not the handler's stay as they are:
response migration and response serialization remain `-32603`,
and request migration remains `-32602`.
Notifications still get no response.

The server-to-client direction of the bidirectional macro is out of scope.
Its generated client-side handlers return `String` errors and send them as `-32000`;
changing that changes their handler type.

Both generated clients drop error detail today.
The HTTP client's `TransportError::JsonRpc { code, message }` gains `data: Option<serde_json::Value>`.
The bidirectional generated client turns every error into `ClientError::internal(String)`;
it gains `ClientError::Rpc(JsonRpcError)` and returns it.

## Explorer

The explorer reads `x-ras-params-schema` and falls back to `params[0].schema`.
It resolves the reference wrapper for field rendering and guessed values,
so a documented field that refers to another struct shows that struct's fields.
For named methods it sends the request object as `params`, which is the by-name call.
Each operation lists its examples with their rendered docs.
Choosing one fills the editors (JSON-RPC params; REST path, query and body),
clears fields the example leaves out,
and shows the expected result or error next to the response.
An operation with examples opens with its first success example in the editors;
one without examples keeps the guessed values.

## Checkpoints

| Checkpoint | Scope | Depends on | Completion criteria |
| --- | --- | --- | --- |
| 1 | Document validation and schema assembly | Baseline | The OpenRPC meta-schema and the OpenAPI 3.0 schema are vendored as test fixtures with their dependencies; validation uses `jsonschema` with network retrieval off, and runs with `--locked`. The schema assembly procedure replaces both normalization passes, with per-contract generation. The documents of the existing spec-test services validate. Fixtures cover a nested type, a recursive root, a generic root, a conflicting name, a type with asymmetric serde renames, and a documented field referring to a struct; tests assert each reference's *target*, not only that it resolves. The explorer resolves the reference wrapper, checked by a Playwright test of the rendered fields and the guessed request. Only incompatibilities these fixtures show are fixed. |
| 2 | OpenRPC parameters by name | 1 | The classifier, named and raw descriptors, `x-ras-params-schema`, the present-`null` deserializer and `{}` for absent `params`. Tests build a by-name call from the descriptors and send it through real dispatch for: `rename_all`, `deny_unknown_fields`, skipped `Option` fields, a flattened struct, a flattened `HashMap<String, Value>`, a flattened enum, `dependencies` (raw), a generic request, a newtype, a root `$ref`, and `()`. Absent `params`, explicit `null`, `{}` and `[]` are tested separately through canonical and legacy dispatch, for an empty struct and an all-optional struct. The explorer reads the new key. The unused placeholder example code is deleted. |
| 3 | JSON-RPC handler errors over HTTP | Baseline | `PublicRpcError`, the predicate and both entry points; canonical and legacy dispatch use them. `TransportError::JsonRpc` carries `data`. Tests: sanitization unchanged for ordinary errors, for an ordinary error wrapping a `JsonRpcError`, and for a `PublicRpcError` wrapped by `thiserror` or `anyhow`; code boundaries `-32768`, `-32000`, `-31999`, `-32602` and a positive code; migration errors unchanged; HTTP status and body checked through the generated client. |
| 4 | Bidirectional handler errors | 3 | Server branches, both router paths and the generated client carry the error, covered by a WebSocket test using both router registration APIs. Notifications still produce no response. |
| 5 | JSON-RPC examples | 2, 3 | The `examples` syntax and its diagnostics, pairings, `x-ras-error-examples`, the method `errors` list, `try_generate_*`, the cached serving path, and the request round trip with its equality check. Tests validate every example against its schema, call `try_generate_*`, and include the lossy-rename case. Compile fixtures cover `feature_gated` client-only builds, native and `wasm32`; a request without `Deserialize` that compiles without examples; and no example code with `openrpc` off. Serving tests cover two services in one process, concurrent first requests, a cached failure, and `docs_require_auth` with cold and warm caches. |
| 6 | REST examples | 1 | The `examples` syntax and its diagnostics, keyed examples on parameters, body and responses, success statuses, `ErrorBody`, `try_generate_*` and the cached serving path, with the same serving tests as checkpoint 5. Example values are validated against the published OpenAPI schemas through a test-only translation of the supported subset to draft-07 (`nullable`, the reference wrapper, component references); an unsupported construct fails the test. Tests cover examples across several statuses, without a body, with omitted optional query values, with `false`, `0` and `""`, and with `None` for an optional primitive and an optional referenced struct. |
| 7 | Explorer examples | 5, 6 | The examples list, filling and clearing the editors, and the expected outcome. Playwright tests in `tests/playwright` check the editors and the outgoing request, using the existing JSON-RPC fixture and a REST one. |
| 8 | Documentation and release | 5, 6, 7 | `documentation/src/macros/*.md`, `generated-specs-and-clients.md` and the tutorial show examples, handler errors and where examples compile. `CHANGELOG.md`, versions, dependency requirements and `Cargo.lock` are updated. The `examples/` crates build and pass their tests. |

Checkpoints 3 and 4 do not depend on the document work and can run first.

## Compatibility and versions

Existing handlers need no change, and wire responses are unchanged until a handler opts in,
apart from absent `params` now decoding as `{}` where `null` failed.
Code that reads `JsonRpcRequest.params` sees `Some(Value::Null)` for an explicit `null`,
where it saw `None`.
The breaks are in the generated documents and in three public error enums.
In both documents, a type whose serialized and deserialized schemas differ
gets a second component, `<Name>.Serialize`, for its response use.

| Crate | Bump | Reason |
| --- | --- | --- |
| `ras-jsonrpc-macro` | 0.3.0 → 0.4.0 | The OpenRPC parameter shape and component schemas change; generated dispatch passes `PublicRpcError` through. |
| `ras-transport-core` | 0.1.0 → 0.2.0 | `TransportError::JsonRpc` gains a field. The enum becomes `#[non_exhaustive]` in the same break. |
| `ras-jsonrpc-bidirectional-server` | 0.3.0 → 0.4.0 | `ServerError` gains a variant and becomes `#[non_exhaustive]`. |
| `ras-jsonrpc-bidirectional-client` | 0.3.0 → 0.4.0 | `ClientError` gains a variant and becomes `#[non_exhaustive]`. |
| `ras-jsonrpc-bidirectional-macro` | 0.2.1 → 0.3.0 | The generated server and client change their error contract. |
| `ras-rest-macro` | 0.3.1 → 0.4.0 | Component schemas change shape under `openapi3()` settings; the rest is additive. |
| `ras-jsonrpc-types` | 0.2.0 → 0.3.0 | `PublicRpcError` and the policy are additive, but explicit `null` params now deserialize to `Some(Value::Null)`, which changes what existing readers of the field see. |
| `ras-api-explorer-assets` | 0.1.0 → 0.1.1 | Additive. |

Code that builds `TransportError::JsonRpc` or matches it without `..` stops compiling.
In this workspace that is the generated client and one test in `ras-transport-core`.
Anything that generated a client from an OpenRPC document gets a different parameter shape;
such a client could not have called the server anyway.
Generated client method signatures, permission-manifest entries,
and the explorer configuration do not change; tests pin all three.
Unpublished `examples/` crates are not bumped.

## Alternatives considered

JSON blocks in doc comments are easy to write, but nothing checks them against the types,
so they drift silently.

Type-level examples through `#[schemars(example = ...)]` describe one type, not a call,
and cannot express a response or an error.
They keep working alongside method examples.

A typed error per method, such as `-> User ! CreateUserError`,
would give error schemas and typed client errors.
It changes every handler signature and both generated clients' error types;
it can be layered on top of `PublicRpcError` later.

Making `JsonRpcError` the opt-in type, and searching `source()` for it, was the first revision.
It lets an upstream error reach the client through an incidental `?`.

Keeping the single `params` descriptor for every method would make every example
show a call the server rejects.

## What it costs

The macro grammars grow by one field with a nested syntax per protocol,
and the parsers grow with them.
The classifier depends on the shape of schemars output,
so a schemars upgrade can move a method between named and raw;
the dispatch tests from checkpoint 2 catch that.

`PublicRpcError` must be returned directly.
A handler that wraps it in its own error type gets the sanitized `-32603`;
that fails safe, and the documentation says so.

Switching to `openapi3()` settings changes existing component schemas,
which is why `ras-rest-macro` takes a minor bump.

Schema assembly is rewritten in both macros in the first checkpoint,
before any example exists.
It is the riskiest part of the change, and the reference-target tests carry it.

Request types with examples need `DeserializeOwned` in every build the document compiles in,
which for JSON-RPC includes client-only and wasm builds.

## Legacy versions

Examples attach to the canonical method or endpoint only.
Legacy versions (inside `versions: [v1 { ... }]`) get none,
and the parser rejects `examples` inside a version block with a message saying so.
Decided on 2026-11-01; adding them later is additive.

## Follow-ups

- Examples on legacy versions.
- A strict document mode that fails generation for raw methods.
- General conversion of custom schemas, recursive compositions, tuples and maps to OpenAPI 3.0,
  beyond what the checkpoint 1 fixtures show.
- Sanitizing server-to-client handler errors in the bidirectional macro.

## Out of scope

Structured error data for REST (`RestError` has only status and message),
OpenRPC output for the bidirectional macro, which has none today,
and examples in the `ras-file-macro` OpenAPI output.

## Round 1 review

Each finding from [answer 1](api-examples-review/answer-1.md), checked against the code, with its ruling.

1. **schemars version.** Taken. `Cargo.lock` resolves 1.2.1; the plan now targets it and validation runs with `--locked`.
2. **Raw fallback describes a failing call.** Taken. The claim that maps, arrays and enums are outside JSON-RPC was wrong; only primitives are. Raw methods stay raw, labelled, with the full schema attached. A strict mode is a follow-up.
3. **Classifier and empty params.** Taken. The classifier is specified, and dispatch tries `{}` when `params` is absent; checked that it decodes a missing `params` as `null` today. Each case gets a dispatch test.
4. **Typed is not wire-correct.** Taken in part. Request examples are round-tripped at generation time, and all examples are validated against their schemas in tests. Response schemas under the serialization contract are a follow-up, because they change existing components; checked that schemars 1.2.1 defaults to `Contract::Deserialize`.
5. **Feature gating.** Simpler version. Checked: JSON-RPC document functions compile at the expansion root, REST ones in the server module. Examples compile exactly where those functions do; no new gate, because that would move an existing public function. Compile fixtures cover client-only native and wasm builds, and the requirement is documented.
6. **Source traversal discloses upstream errors.** Taken. `PublicRpcError`, top-level downcast only; `JsonRpcError` does not implement `Error`.
7. **Every handler boundary.** Taken for the HTTP canonical and legacy paths, the bidirectional server and both router paths. Migration and serialization failures keep their current codes. The server-to-client direction is excluded and listed as a follow-up, because its handler type returns `String`.
8. **Example code breaks the reserved rule.** Taken. The example uses `1001`, the macro applies the runtime rule to literals, and HTTP 200 is stated. Checked that nothing in the workspace refreshes tokens on `-32003`; the reason for the rule now names the HTTP status mapping instead.
9. **Bounded checkpoint 1.** Taken as written, with `draft07()` and `openapi3()` settings, which exist in schemars 1.2.1.
10. **ErrorBody and statuses.** Taken. `ErrorBody` covers handler errors only, success statuses get entries, `204` takes no body, and non-`Option` query parameters are required.
11. **Fallible generation.** Taken. Checked that the served documents are regenerated per request today; they move to a `OnceLock`.
12. **Release inventory.** Taken. `try_generate_*` is additive; the gating stays; manifest, client signatures and explorer configuration are pinned by tests. `ras-rest-macro` moves to a minor bump because of the `openapi3()` settings.
13. **Missing tests.** Taken into checkpoints 2 to 7, with Playwright for the explorer. Rejected: example names as OpenRPC component keys; examples live in methods, not `components`, and Rust identifiers are valid names there. Error deduplication is specified.

Ruling 4 is superseded by round 2: response schemas use the serialization contract in this change.

## Round 2 review

Each finding from [answer 2](api-examples-review/answer-2.md), checked against the code, with its ruling.
This was the last round; every blocker below is taken, and its tests are part of the build.

1. **Decoding is not enough.** Taken. The round trip serializes twice and requires equality, and every named example key needs a descriptor.
2. **Response schemas.** Taken. Responses use the serialization contract now; components are keyed by type and contract, with `<Name>.Serialize` when the schemas differ. This replaces the round 1 follow-up.
3. **New `Deserialize` bound.** Taken. `DeserializeOwned` on request types with examples is stated and covered by compile fixtures.
4. **Dynamic keys and cross-field constraints.** Taken. `additionalProperties` must be absent or `false`; `dependencies`, `propertyNames`, `minProperties` and `maxProperties` make a request raw; a flattened `HashMap<String, Value>` fixture is added.
5. **Present `null`.** Taken. Checked that `JsonRpcRequest.params` is `Option<Value>`. A present `null` deserializes to `Some(Value::Null)` with the field type unchanged, and only a missing field gets `{}`. `ras-jsonrpc-types` moves to a minor bump for it.
6. **Schema assembly.** Taken. Checked that the JSON-RPC pass handles only `$defs`, and that the REST pass strips `definitions` from any nested object. The procedure is now its own section, and the tests assert reference targets.
7. **Reference wrappers.** Taken. Checked that `openapi3()` includes `RemoveRefSiblings` and that the explorer resolves only direct `$ref`s. The single-reference wrapper is read as a reference by the classifier and the explorer, tested in checkpoint 1.
8. **`nullable` in example validation.** Taken. OpenAPI example values are validated through a test-only translation of the supported subset, with `None` cases.
9. **Cache identity and authorization.** Taken. A function-local cache per document handler, caching failure, generation output only, behind the existing authorization, with the tests listed in checkpoints 5 and 6.
10. **Conversion and code validation.** Taken. One predicate, two entry points. The "`JsonRpcError` returned as an error" test becomes an ordinary error wrapping a `JsonRpcError`; no compile-fail test, since the workspace has no `trybuild` and the case cannot compile anyway.

## Build notes

Where the build departs from this plan, and why.

**Checkpoint 1, schema assembly.**
The four-step procedure in "Schema assembly" is replaced by a simpler one.
Each document uses one schemars generator per contract and asks it for every request and response type
with `subschema_for`, instead of generating a root schema per type.
The generator then keeps names unique itself (`Account`, `Account2`),
writes references that already point at `#/components/schemas/`,
and never produces a root `#` reference, so steps 2 to 4 have nothing left to relocate or rebase.
What remains is merging the two contracts, with `<Name>.Serialize` for names whose schemas differ,
repeated until types that only refer to a differing type are caught too.

That merge lives in the new crate `ras-spec-components`.
A document function runs in the consumer's crate, and for JSON-RPC in client and wasm builds as well,
so both macros embed the crate's source instead of depending on it at runtime.
Validation lives in the unpublished `ras-spec-conformance`, a path-only dev-dependency of both macros.

Under the serialization contract, schemars marks `Option` fields without `skip_serializing_if` as required,
because they are always sent.
Most response types with an `Option` field therefore get a `.Serialize` component.
That is accurate, and the explorer hides the suffix.

The explorer's guessed-value function had a cycle guard that never fired,
so a recursive request type recursed without end; it is fixed in the same checkpoint.

**Checkpoint 3, handler errors.**
Wrapping is stricter than the plan expected in one case.
`anyhow::Error` converts into `Box<dyn Error>` as anyhow's own wrapper type,
so a `PublicRpcError` passed through `anyhow` is hidden even without added context.
That fails safe and matches the rule that only the returned error counts; the test pins it.

**Checkpoint 4, WebSocket handler errors.**
A failing notification, a request without `id`, already gets an error response with `"id": null`,
and a succeeding one gets a success response.
JSON-RPC says a notification gets no response at all.
This predates the plan and this change keeps it as it is; it is left for a separate fix.
The same is true over HTTP for a request with `"id": null`,
which `JsonRpcRequest` cannot tell apart from a missing `id`.

**Checkpoints 5 to 8, examples, explorer and release.**
`SpecGenerationError` lives in `ras-spec-components` as an ordinary type, re-exported by
`ras-jsonrpc-types` and `ras-rest-core`; the served documents cache it directly, so no
separate `CachedSpecError` exists.
The REST error body component is named `RestErrorBody`, not `ErrorBody`,
because `ErrorBody` is a likely name for a user's own type; the name is reserved and a clash fails generation.
A 204 example has no response content to sit on and is recorded under `x-ras-examples` on the 204 response.
The embedded source must compile in any edition, so `ras-spec-components` builds as edition 2021
and its tests catch syntax such as let chains.
REST examples appear in name order in the explorer, because OpenAPI keys them by name in JSON objects.
The version table grew by three crates that expose a bumped crate's types:
`ras-jsonrpc-core`, `ras-jsonrpc-bidirectional-types` and `ras-file-macro`.
No compile-fail fixtures exist (the workspace has no `trybuild`);
the diagnostics are unit-tested through the parsers instead,
and a request type without `Deserialize` in a client-only build is covered only by the documentation.

