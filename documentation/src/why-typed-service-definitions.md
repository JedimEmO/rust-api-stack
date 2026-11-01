# Why typed service definitions

The API boundary is where a service and its clients drift apart.
The drift has a few familiar shapes:

- server handlers accept one shape while clients send another;
- documentation falls behind the real implementation;
- auth requirements live in middleware or comments instead of the operation
  definition;
- file uploads buffer too much data or validate too late;
- renamed fields break older clients without an explicit migration path.

RAS service macros attack this by being strict.
They ask you to describe every endpoint with concrete Rust request and response types,
then generate the repetitive boundary code from that description.

The service definition is the source of truth.
The generated trait forces every declared endpoint to be implemented.
Request and response types are serialized through `serde`,
described through `schemars` where specs are generated, and reused by the generated clients.
A protected operation hands its handler the authenticated user,
so handler code can depend on the caller's identity without parsing tokens.

The macros do not remove runtime validation.
They move the easy-to-forget plumbing into generated code,
and leave the service implementation focused on domain behaviour:
read typed input, apply business rules, return typed output.

## What the macros generate

Depending on the macro and the enabled features, a service definition can generate:

- a trait that lists every handler method with typed parameters;
- an Axum router or WebSocket service;
- authentication and permission checks that run before the handler;
- native Rust clients, which also compile to WASM for browser use;
- OpenRPC or OpenAPI documents with schemas, auth metadata and authored examples;
- an API explorer page for JSON-RPC and REST services;
- routes or methods that serve older payload shapes, for macros that support versioned migrations.

The result is a narrower contract between API design, server implementation,
client usage and published documentation.
