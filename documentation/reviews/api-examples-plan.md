Plan, 2026-11-01. Based on `15404c3`.
Adds authored request/response examples and error examples to the OpenRPC and OpenAPI documents
the service macros generate, and lets JSON-RPC handlers return errors a client can act on.
Two fixes come with it, because the examples would expose them:
the OpenRPC description of `params` and the missing error schemas in OpenAPI.

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
schemars also writes `examples` arrays into component schemas, a key OpenAPI 3.0 does not allow there.

Nothing checks the generated documents against the OpenRPC or OpenAPI schemas,
so none of this was caught.

## Decisions

Examples are typed Rust values written in the macro.
The generated code assigns each value to the declared request or response type,
so an example that drifts from its type fails to compile.

The OpenRPC document describes JSON-RPC parameters by name,
one content descriptor per top-level request field.

A handler opts into a wire error by returning `ras_jsonrpc_types::JsonRpcError`.
The handler signature keeps `Box<dyn Error + Send + Sync>`,
so existing handlers compile and behave as before.

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
            error: { code: -32010, message: "Email already registered", data: EmailTaken { field: "email".into() } },
        },
    ],
}
```

An example has exactly one of `result` and `error`.
`params` is required unless the request type is `()`.
`code` must be an integer literal and `message` a string literal, so the macro can check them;
`data` is any expression whose type implements `Serialize`.

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

Every path parameter must be given; optional query parameters may be left out.
`body` is required exactly when the endpoint declares a request type.
A success example may set `status: 201`, defaulting to 200.
An error `status` must be a 4xx or 5xx literal.

The macro reports a missing or unknown field, a duplicate example name,
and a `result` together with an `error`, at the offending tokens.
Values are emitted with `quote_spanned!` into typed bindings,
so a type mismatch points at the example, not at the macro call.

Using examples requires the types to implement `Serialize`.
That costs nothing in practice: request types already do for the generated clients.

## Generated OpenRPC

A method whose request schema is a plain object
gets `"paramStructure": "by-name"` and one content descriptor per property.
`required` comes from the schema's `required` list,
and the description from the property, which carries the field's doc comment.
The whole request type stays reachable through `x-ras-params-schema` with a `$ref`,
so the explorer does not rebuild it from the fields.

A request schema that is not a plain object
(a primitive, an array, an enum, a top-level `oneOf`, `anyOf` or `allOf`, or a map)
keeps the single `params` descriptor and is marked `x-ras-params-raw: true`.
JSON-RPC 2.0 requires `params` to be an object or an array,
so these request types were already outside the protocol;
the marker says so instead of describing a call that fails.
A `()` request has no parameters.

Success examples become standard Example Pairing objects.
`params` holds one Example per key of the serialized request, or one `params` Example for the raw case,
and `result` holds the serialized response.

An Example Pairing has no place for an error.
Error examples go into `x-ras-error-examples` on the method,
each with `name`, `summary`, `description`, `params` and an `error` object of `code`, `message` and `data`.

Each method also gets the standard `errors` list.
It references the built-in errors in `components.errors` that the method can produce,
from `-32602` for every method that takes parameters
to the authentication and permission errors for `WITH_PERMISSIONS`.
Application errors from the error examples are added and deduplicated by code;
the first message for a code wins.

## Generated OpenAPI

OpenAPI 3.0 has no request/response pairs either.
An example appears under the same key in each place it touches:
`parameters[].examples`, `requestBody.content."application/json".examples`
and `responses.<status>.content."application/json".examples`.
Swagger UI and Redoc show the parts side by side under that name,
and the explorer pairs them by key.

A new `ErrorBody` component schema (`{ "error": string }`) describes every error response.
Statuses that only error examples use, such as 404 or 409, are added to the operation.

Component schemas have their `examples` array replaced by `example` (the first item),
which OpenAPI 3.0 accepts.

## Handler errors at runtime

`JsonRpcError` gets `Display` and `std::error::Error`.
A handler returns it through `?` or `Err(JsonRpcError::new(code, message, data).into())`.

Generated dispatch looks for a `JsonRpcError` before sanitizing:
first by downcasting the box, then by walking `source()`.
When one is found, its code, message and data go to the client as written.
Anything else becomes the sanitized `-32603`, as today.

Handler errors may use any code outside the reserved range `-32768..=-32000`, and `-32602`.
Other reserved codes are logged and replaced by `-32603`.
Clients act on the built-in codes, for instance by refreshing a token on `-32003`,
and a handler must not trigger that by accident.

The bidirectional macro maps handler errors to `ServerError::Internal(String)`,
losing the type before the router sees it.
It needs the same lookup in the generated server, a `ServerError::Rpc(JsonRpcError)` variant,
and a router that sends that variant unchanged.

Both generated clients drop error detail.
The HTTP client's `TransportError::JsonRpc { code, message }` has no `data`,
and the bidirectional client turns every error into `ClientError::internal(String)`.
`TransportError::JsonRpc` gains `data: Option<serde_json::Value>`,
and the bidirectional client gains `ClientError::Rpc(JsonRpcError)`.

## Explorer

The explorer reads `x-ras-params-schema` and falls back to `params[0].schema`.
Each operation lists its examples with their rendered docs.
Choosing one fills the editors (JSON-RPC params; REST path, query and body)
and shows the expected result or error next to the response.
An operation with examples opens with its first success example in the editors;
one without examples keeps the guessed values.

## Checkpoints

| Checkpoint | Scope | Depends on | Completion criteria |
| --- | --- | --- | --- |
| 1 | Document validation tests | Baseline | The generated documents of the existing spec-test services validate against the OpenRPC meta-schema and the OpenAPI 3.0 schema, vendored as test fixtures, with `jsonschema` as a dev-dependency. Existing violations are fixed here, including the `examples` key in OpenAPI component schemas. |
| 2 | OpenRPC parameters by name | 1 | By-name descriptors, the raw fallback and `x-ras-params-schema` are emitted. The explorer reads the new key. The unused placeholder example code is deleted. |
| 3 | JSON-RPC handler errors over HTTP | Baseline | `JsonRpcError` implements `Error`; dispatch sends handler-chosen errors and enforces the reserved range. `TransportError::JsonRpc` carries `data`. The sanitization tests still pass, with new tests for an opted-in error, a reserved code, and an error wrapped by `thiserror` and by `anyhow`. |
| 4 | Bidirectional handler errors | 3 | The generated WebSocket server and client carry `JsonRpcError` end to end, covered by a WebSocket test. |
| 5 | JSON-RPC examples | 2, 3 | The `examples` syntax with its diagnostics, example pairings, `x-ras-error-examples` and the method `errors` list. Every example value validates against its schema in a test. |
| 6 | REST examples | 1 | The `examples` syntax with its diagnostics, keyed examples on parameters, body and responses, and `ErrorBody`. Every example value validates against its schema in a test. |
| 7 | Explorer examples | 5, 6 | The examples list, filling the editors, and the expected outcome, covered by the explorer tests. |
| 8 | Documentation and release | 5, 6, 7 | `documentation/src/macros/*.md`, `generated-specs-and-clients.md` and the tutorial show examples and handler errors. `CHANGELOG.md` and versions are updated. |

Checkpoints 3 and 4 do not depend on the document work and can run first.
Checkpoint 4 can be dropped from this change if the WebSocket path should wait,
at the price of the two transports treating handler errors differently.

## Compatibility and versions

Existing handlers need no change, and wire responses are unchanged until a handler opts in.
The breaks are in the generated documents and in three public error enums.

| Crate | Bump | Reason |
| --- | --- | --- |
| `ras-jsonrpc-macro` | 0.3.0 → 0.4.0 | The OpenRPC parameter shape changes; generated dispatch passes handler errors through. |
| `ras-transport-core` | 0.1.0 → 0.2.0 | `TransportError::JsonRpc` gains a field. The enum becomes `#[non_exhaustive]` in the same break. |
| `ras-jsonrpc-bidirectional-server` | 0.3.0 → 0.4.0 | `ServerError` gains a variant and becomes `#[non_exhaustive]`. |
| `ras-jsonrpc-bidirectional-client` | 0.3.0 → 0.4.0 | `ClientError` gains a variant and becomes `#[non_exhaustive]`. |
| `ras-jsonrpc-bidirectional-macro` | 0.2.1 → 0.3.0 | The generated server and client change their error contract. |
| `ras-rest-macro` | 0.3.1 → 0.3.2 | Additive syntax and document content; the `example` conversion is a fix. |
| `ras-jsonrpc-types` | 0.2.0 → 0.2.1 | `JsonRpcError` implements `Error`. |
| `ras-api-explorer-assets` | 0.1.0 → 0.1.1 | Additive. |

Code that builds `TransportError::JsonRpc` or matches it without `..` stops compiling.
In this workspace that is the generated client and one test in `ras-transport-core`.
Anything that generated a client from an OpenRPC document gets a different parameter shape;
such a client could not have called the server anyway.

## Alternatives considered

JSON blocks in doc comments are easy to write, but nothing checks them against the types,
so they drift silently.

Type-level examples through `#[schemars(example = ...)]` already flow into component schemas.
They describe one type, not a call, and cannot express a response or an error.
They keep working alongside method examples.

A typed error per method, such as `-> User ! CreateUserError`,
would give error schemas and typed client errors.
It changes every handler signature and both generated clients' error types;
it can be layered on top of the opt-in error later.

Keeping the single `params` descriptor would make every example
show a call the server rejects.

## What it costs

The macro grammars grow by one field with a nested syntax per protocol,
and the parsers grow with them.
Building the by-name descriptors happens at document generation time
and depends on the shape of schemars output,
so a schemars upgrade that changes how structs are emitted can move a method into the raw fallback.
The validation tests from checkpoint 1 catch that.

The downcast finds a `JsonRpcError` only when it or its `source()` chain exposes it.
A wrapper that hides its inner error turns an opted-in error into `-32603`.
That fails safe, but it can surprise; the documentation says to return the error directly.

## Open questions

Serializing an example can fail at document generation time,
for instance for a map with non-string keys.
Panicking with the method and example name is the simplest answer,
but the document is also generated at runtime for `openrpc.json`; open.

Examples on legacy versions (inside `versions: [v1 { ... }]`) are left out of the first version.
Whether they are needed is open.

The reserved-range rule allows `-32602` from handlers for validation failures.
Whether other built-in codes should be allowed is open.

## Out of scope

Structured error data for REST (`RestError` has only status and message),
OpenRPC output for the bidirectional macro, which has none today,
and examples in the `ras-file-macro` OpenAPI output.
