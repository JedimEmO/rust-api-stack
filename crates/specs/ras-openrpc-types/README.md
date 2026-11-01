# ras-openrpc-types

Rust types for [OpenRPC 1.3.2](https://spec.open-rpc.org/) documents,
with serde support, `bon` builders and validation helpers.

Use it to build, read or check an OpenRPC document in Rust.
The RAS service macros do not use this crate;
`jsonrpc_service!` writes its OpenRPC document as `serde_json::Value`.
The crate stands on its own and depends only on `serde`, `serde_json` and `bon`.

## Installation

```toml
[dependencies]
ras-openrpc-types = "0.1"
```

## Building a document

```rust
use ras_openrpc_types::{
    ContentDescriptor, Info, Method, OpenRpc, ParameterStructure, Schema, Validate,
};

let user_id = ContentDescriptor::new("userId", Schema::string())
    .with_description("The user ID")
    .required();

let user = ContentDescriptor::new(
    "user",
    Schema::object()
        .with_property("id", Schema::string().with_format("uuid"))
        .with_property("name", Schema::string().with_min_length(1))
        .require_property("id")
        .require_property("name"),
);

let get_user = Method::new("getUser", vec![user_id.into()])
    .with_summary("Get a user by ID")
    .with_result(user.into())
    .with_param_structure(ParameterStructure::ByName);

let document = OpenRpc::v1_3_2(
    Info::new("Example API", "1.0.0").with_description("A small example API"),
    vec![get_user.into()],
);

document.validate().expect("valid OpenRPC document");
let json = serde_json::to_string_pretty(&document).unwrap();
```

Notice the `.into()` calls.
Wherever OpenRPC allows a Reference Object in place of a value,
the field holds an enum such as `ContentDescriptorOrReference` or `MethodOrReference`,
and `From` converts a value or a `Reference` into it.
`validate` comes from the `Validate` trait, which has to be in scope.

Every object also derives a `bon` builder, for instance `Method::builder()`.
The builders take field types exactly, so string fields need a `String`, not a `&str`;
the `new` and `with_*` methods above take `impl Into<String>` and are shorter to write.

The [`basic_usage` example](examples/basic_usage.rs) builds a document with components and reads it back.

## Types

| Area | Types |
|---|---|
| Document | `OpenRpc`, `Info`, `Contact`, `License`, `Server`, `ServerVariable` |
| Methods | `Method`, `ContentDescriptor`, `ParameterStructure`, `ErrorObject` |
| Schemas | `Schema`, `SchemaType`, a JSON Schema draft 7 model |
| Reuse | `Components`, `Reference` |
| Examples and links | `Example`, `ExamplePairing`, `Link`, `RuntimeExpression` |
| Documentation | `Tag`, `ExternalDocumentation` |
| Extensions | `Extensions`, the `x-*` fields every object can carry |

The `version` module lists the OpenRPC versions the crate accepts, from `1.0.0` to `1.3.2`.

## Validation

`Validate::validate` checks the constraints a document can break without being malformed JSON:

- the `openrpc` version is one the crate supports, and `info` has a title and a version;
- method names are unique, contain no spaces, and do not use the reserved `rpc.` prefix;
- parameter names within a method are unique, and required parameters come before optional ones;
- error codes within a method are unique,
  and a code in the JSON-RPC reserved range `-32768..=-32000` is one of the pre-defined codes
  or a server error code in `-32099..=-32000`;
- component keys match `^[a-zA-Z0-9.\-_]+$`, and references name a known component type;
- schema bounds are consistent, such as `minimum` not above `maximum`;
- an example does not set both `value` and `externalValue`;
- extension keys start with `x-`.

Some checks are deliberately shallow.
URLs and email addresses get a shape check, not a full parse.
A reference is checked for its form; it is not resolved against `components`.
Example values are not checked against their schemas.

A failure is an `OpenRpcError`.
Most checks return `ValidationError`, with a `field_path` such as `methods[0]` when the problem sits in a nested object;
others are `MissingField`, `DuplicateKey`, `InvalidUrl`, `InvalidEmail` or `UnsupportedVersion`.
`OpenRpcResult<T>` is `Result<T, OpenRpcError>`.

## Checks

```bash
cargo test -p ras-openrpc-types --locked
cargo clippy -p ras-openrpc-types --all-targets --all-features --locked -- -D warnings
```

## License

This project is licensed under either MIT or Apache-2.0. See
[LICENSE-MIT](../../../LICENSE-MIT) and [LICENSE-APACHE](../../../LICENSE-APACHE).
