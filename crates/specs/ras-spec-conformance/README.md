# ras-spec-conformance

Test support for the service macros.
It checks a generated OpenRPC or OpenAPI document against the official schema for its format,
and checks that every local `$ref` in it points at something.

A document can pass these checks and still describe a call the server rejects;
the macro test suites cover that separately.

This crate is not published. The macro crates use it as a path-only dev-dependency.

## Vendored schemas

The schemas live in `schemas/` and are loaded from disk at compile time,
so validation never touches the network.

| File | Source | License |
| --- | --- | --- |
| `openrpc-1.3.json` | npm `@open-rpc/meta-schema` 1.14.9, the `openrpcDocument` export | Apache-2.0 |
| `json-schema-tools-meta.json` | npm `@json-schema-tools/meta-schema` 1.7.5, `schema.json` | Apache-2.0 |
| `openapi-3.0.json` | `https://spec.openapis.org/oas/3.0/schema/2021-09-28` | Apache-2.0 |

The OpenRPC meta-schema refers to `https://meta.json-schema.tools` for its schema objects;
`json-schema-tools-meta.json` is registered under that URI.
Both declare a custom `$schema`, so they are evaluated as JSON Schema draft 7,
the dialect they are written in.
