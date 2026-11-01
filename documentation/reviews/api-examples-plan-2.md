Plan, revision 2, 2026-11-01. Based on `15404c3`.
Revises [revision 1](api-examples-plan.md) after the first review
([brief](api-examples-review/brief-1.md), [answer](api-examples-review/answer-1.md));
the rulings are in the last section.

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
Document generation therefore round-trips every request example,
and tests validate every example against its schema.

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
For a JSON-RPC service that means example values, and `Serialize` on their types,
must be available in client builds as well.
Moving the JSON-RPC document functions behind a new gate would change
where an existing public function exists, which this change does not need;
the requirement is documented instead.

## Generated OpenRPC

### Parameters

The classifier resolves local `$ref`s at the root of the request schema, with cycle detection.
A request schema is *named* when the result has `type: "object"`, a finite `properties` map,
and no top-level `oneOf`, `anyOf`, `allOf`, `not`, `if`,
`patternProperties` or schema-valued `additionalProperties`.
`additionalProperties: false`, from `deny_unknown_fields`, is allowed.
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

### Examples and errors

Success examples become standard Example Pairing objects.
`params` holds one Example per key of the serialized request, for named methods,
or one `params` Example for raw ones; a skipped `None` field has no entry.
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
The served `openrpc.json` and `openapi.json` are generated once, on first request,
and kept in a `OnceLock`; on failure they answer with a generic 500 and log the context.
Today they regenerate the document on every request.

## Handler errors at runtime

`ras-jsonrpc-types` gains `PublicRpcError { code, message, data }`,
which implements `Display` and `std::error::Error`.
A handler returns it with `Err(PublicRpcError::new(1001, "Email already registered").with_data(...).into())`.
`JsonRpcError` stays a wire type and does not implement `Error`.

The policy sits in one function in `ras-jsonrpc-types`, used at every handler error boundary.
It downcasts the handler's box to `PublicRpcError` and nothing else; it does not walk `source()`.
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
| 1 | Document validation, bounded | Baseline | The OpenRPC meta-schema and the OpenAPI 3.0 schema are vendored as test fixtures with their dependencies; validation uses `jsonschema` with network retrieval off, and runs with `--locked`. The documents of the existing spec-test services validate. Generation uses `SchemaSettings::draft07()` for OpenRPC and `SchemaSettings::openapi3()` for OpenAPI, replacing the hand-written normalization those settings cover. Only incompatibilities these fixtures show are fixed; conversion only ever touches schema objects, never example values. A separate test asserts every local `$ref` resolves. |
| 2 | OpenRPC parameters by name | 1 | The classifier, named and raw descriptors, `x-ras-params-schema`, and `{}` for absent `params`. Tests build a by-name call from the descriptors and send it through real dispatch for: `rename_all`, `deny_unknown_fields`, skipped `Option` fields, a flattened struct, a flattened map and a flattened enum (raw), a generic request, a newtype, a root `$ref`, an empty struct with `params` omitted, `{}` and `[]`, and `()`. The explorer reads the new key. The unused placeholder example code is deleted. |
| 3 | JSON-RPC handler errors over HTTP | Baseline | `PublicRpcError` and the policy function; canonical and legacy dispatch use it. `TransportError::JsonRpc` carries `data`. Tests: sanitization unchanged for ordinary errors, a `JsonRpcError` returned as an error, and a `PublicRpcError` wrapped by `thiserror` or `anyhow`; code boundaries `-32768`, `-32000`, `-31999`, `-32602` and a positive code; migration errors unchanged; HTTP status and body checked through the generated client. |
| 4 | Bidirectional handler errors | 3 | Server branches, both router paths and the generated client carry the error, covered by a WebSocket test using both router registration APIs. Notifications still produce no response. |
| 5 | JSON-RPC examples | 2, 3 | The `examples` syntax and its diagnostics, pairings, `x-ras-error-examples`, the method `errors` list, `try_generate_*`, the `OnceLock` serving path, and the request round trip. Tests validate every example against its schema and call `try_generate_*`. Compile fixtures cover `feature_gated` client-only builds, native and `wasm32`. |
| 6 | REST examples | 1 | The `examples` syntax and its diagnostics, keyed examples on parameters, body and responses, success statuses, `ErrorBody`, `try_generate_*` and the `OnceLock` serving path. Tests cover examples across several statuses, without a body, with omitted optional query values, and with `false`, `0` and `""` values. |
| 7 | Explorer examples | 5, 6 | The examples list, filling and clearing the editors, and the expected outcome. Playwright tests in `tests/playwright` check the editors and the outgoing request, using the existing JSON-RPC fixture and a REST one. |
| 8 | Documentation and release | 5, 6, 7 | `documentation/src/macros/*.md`, `generated-specs-and-clients.md` and the tutorial show examples, handler errors and where examples compile. `CHANGELOG.md`, versions, dependency requirements and `Cargo.lock` are updated. The `examples/` crates build and pass their tests. |

Checkpoints 3 and 4 do not depend on the document work and can run first.

## Compatibility and versions

Existing handlers need no change, and wire responses are unchanged until a handler opts in,
apart from absent `params` now decoding as `{}` where `null` failed.
The breaks are in the generated OpenRPC document and in three public error enums.

| Crate | Bump | Reason |
| --- | --- | --- |
| `ras-jsonrpc-macro` | 0.3.0 → 0.4.0 | The OpenRPC parameter shape changes; generated dispatch passes `PublicRpcError` through. |
| `ras-transport-core` | 0.1.0 → 0.2.0 | `TransportError::JsonRpc` gains a field. The enum becomes `#[non_exhaustive]` in the same break. |
| `ras-jsonrpc-bidirectional-server` | 0.3.0 → 0.4.0 | `ServerError` gains a variant and becomes `#[non_exhaustive]`. |
| `ras-jsonrpc-bidirectional-client` | 0.3.0 → 0.4.0 | `ClientError` gains a variant and becomes `#[non_exhaustive]`. |
| `ras-jsonrpc-bidirectional-macro` | 0.2.1 → 0.3.0 | The generated server and client change their error contract. |
| `ras-rest-macro` | 0.3.1 → 0.4.0 | Component schemas change shape under `openapi3()` settings; the rest is additive. |
| `ras-jsonrpc-types` | 0.2.0 → 0.2.1 | `PublicRpcError` and the policy function are additive. |
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

## Open questions

Examples on legacy versions (inside `versions: [v1 { ... }]`) are left out.
Whether they are needed is open.

## Follow-ups

- Response schemas under schemars' serialization contract.
  Both generators use the default deserialization contract for responses too.
  An example the schema validation rejects for this reason is fixed in the type, or waits for this.
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
