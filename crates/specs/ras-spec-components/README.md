# ras-spec-components

The code that turns schemars output into the `components.schemas` of a generated
OpenRPC or OpenAPI document.
`ras-jsonrpc-macro` and `ras-rest-macro` embed it in the code they generate.

The macros cannot call this code as a library.
A document function runs in the consumer's crate, against the consumer's `schemars`,
and for JSON-RPC it also compiles in client-only and wasm builds.
Embedding the source keeps one implementation without adding a dependency
the consumer would have to declare.

`SOURCE` holds `src/components.rs` as text; the crate's own tests compile the same file.
The embedded code needs `schemars` 1.2 or later and `serde_json` in the consumer's crate,
which the generated document functions already require.

## What it does

Each document uses two schemars generators:
the deserialization contract for what the server reads (request bodies, parameters)
and the serialization contract for what it sends (responses).
Every request and response type is generated as a subschema,
so named types become shared components and references point at `#/components/schemas/<name>`.

The two generators then merge into one `components.schemas` map.
A name whose two schemas are equal becomes one component.
A name whose schemas differ, for instance because of `#[serde(rename(serialize = ..., deserialize = ...))]`,
keeps its deserialization schema under the name
and gets its serialization schema as `<name>.Serialize`;
references from the serialization side are rewritten to match.
Two different types that share a schemars name, one only read and one only sent,
also end up as `<name>` and `<name>.Serialize`.
The references are right; the name only suggests a closer relation than there is.
