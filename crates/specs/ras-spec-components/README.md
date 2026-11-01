# ras-spec-components

The code that turns `schemars` output into the `components.schemas` of a generated
OpenRPC or OpenAPI document.
`ras-jsonrpc-macro` and `ras-rest-macro` use it;
you do not add this crate yourself.

## Why the crate ships its own source

The obvious shape would be a library that the generated code calls.
That does not work here.
The generated document functions run in *your* crate,
take the request and response types from your crate,
and call your crate's `schemars` to describe them.
For JSON-RPC they also compile in client-only and wasm builds.
A library call would add a dependency that every API crate has to declare.

So the crate exposes `SOURCE`, the text of `src/components.rs`.
At macro expansion time, the macros parse that text
and paste it into each service's generated code as a private module.
The code then compiles as part of your crate, against your `schemars`, `serde` and `serde_json`.
The generated document functions need those three anyway; the embedded code needs `schemars` 1.2 or later.
The crate itself compiles the same file in its tests,
so there is one implementation, tested in one place.

This has a cost.
Pasted code compiles under *your* crate's edition, not this crate's.
`components.rs` therefore uses only `std`, `schemars`, `serde` and `serde_json`, always through absolute paths,
and only syntax that edition 2018 accepts.
It imports `TryFrom` explicitly, which edition 2018 does not have in its prelude,
and it avoids `let` chains, which need edition 2024.
The crate is built with edition 2021, not 2024, so its own tests reject `let` chains and other 2024-only syntax.

The crate also exports one ordinary type, `SpecGenerationError`,
which the generated `try_generate_*` functions return.
`ras-jsonrpc-types` and `ras-rest-core` re-export it, so you reach it through a crate you already use.

## What the embedded code does

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

For JSON-RPC the embedded code also decides how a request's `params` are described
(named descriptors, or one raw descriptor), checks that a request example reads back unchanged,
and turns authored examples into example pairings, `x-ras-error-examples` and the method's `errors` list.
For REST it describes every error response with the shared `RestErrorBody` schema, `{ "error": "<message>" }`,
and places each authored example under the same key in the operation's parameters, request body and response.
