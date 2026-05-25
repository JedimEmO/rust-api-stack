# Why Typed Service Definitions

RAS service macros are intentionally strict. They ask you to describe endpoints
with concrete Rust request and response types, then generate the repetitive
boundary code from that description.

This matters for API builders because the API boundary is where drift usually
appears:

- server handlers accept one shape while clients send another;
- documentation falls behind the real implementation;
- auth requirements live in middleware or comments instead of the operation
  definition;
- file uploads buffer too much data or validate too late;
- renamed fields break older clients without an explicit migration path.

With RAS, the service definition is the source of truth. The generated trait
forces every declared endpoint to be implemented. Request and response types are
serialized through `serde`, documented through `schemars` where specs are
generated, and reused by generated clients. When an endpoint is protected, the
generated trait signature receives an `AuthenticatedUser`, so handler code can
depend on authenticated identity without repeating token parsing.

The macros do not remove runtime validation. They move the easy-to-forget
plumbing to generated code and leave the service implementation focused on
domain behavior: read typed input, apply business rules, return typed output.

## What The Macros Generate

Depending on the macro and enabled features, a service definition can generate:

- a trait that lists every handler method with typed parameters;
- an Axum router, WebSocket service, or runtime adapter;
- authentication and permission checks before handlers run;
- native Rust clients, including WASM-compatible clients for browser use;
- OpenRPC or OpenAPI documents with schemas and auth metadata;
- explorer UIs for supported HTTP services;
- version compatibility routes or methods where the macro supports migrations.

The result is a narrower contract between API design, server implementation,
client usage, and published documentation.
