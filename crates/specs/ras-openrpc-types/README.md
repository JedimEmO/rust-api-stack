# OpenRPC Types

Rust types for the OpenRPC 1.3.2 specification with serde support, bon builders, and runtime validation helpers.

## Features

- **OpenRPC 1.3.2 specification types** - Objects and fields from the specification
- **Serde serialization/deserialization** - Full JSON support with proper field naming
- **Bon builder patterns** - Ergonomic API construction with type-safe builders
- **Validation helpers** - Checks OpenRPC specification constraints
- **JSON Schema Draft 7 compatibility** - For Schema objects used in OpenRPC documents
- **Reference resolution support** - For $ref within components
- **Specification extensions** - Support for x-* extension fields
- **Type safety** - Helps construct OpenRPC documents with strongly typed Rust values

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
ras-openrpc-types = "0.1.1"
```

## Example Usage

```rust
use ras_openrpc_types::{OpenRpc, Info, Method, ContentDescriptor, Schema};

// Create an OpenRPC document
let openrpc = OpenRpc::builder()
    .openrpc("1.3.2")
    .info(
        Info::builder()
            .title("Example API")
            .version("1.0.0")
            .description("A simple example API")
            .build()
    )
    .methods(vec![
        Method::builder()
            .name("getUser")
            .params(vec![
                ContentDescriptor::new("userId", Schema::string())
                    .with_description("The user ID")
                    .required()
                    .into()
            ])
            .result(
                ContentDescriptor::new("user", Schema::object()
                    .with_property("id", Schema::string())
                    .with_property("name", Schema::string())
                    .with_property("email", Schema::string())
                ).into()
            )
            .build()
            .into()
    ])
    .build();

// Validate the document
openrpc.validate().expect("Valid OpenRPC document");

// Serialize to JSON
let json = serde_json::to_string_pretty(&openrpc).unwrap();
println!("{}", json);
```

## Core Types

### OpenRPC Document Structure

- **`OpenRpc`** - Root object containing the entire specification
- **`Info`** - Metadata about the API (title, version, description, etc.)
- **`Server`** - Server connectivity information with variable substitution
- **`Method`** - JSON-RPC method definitions with parameters and results
- **`Components`** - Reusable components (schemas, examples, errors, etc.)

### Content and Schema Types

- **`ContentDescriptor`** - Describes parameters and results with schemas
- **`Schema`** - JSON Schema Draft 7 compliant schema definitions
- **`Example`** - Example values with embedded or external references
- **`ExamplePairing`** - Request/response example pairs

### Linking and Documentation

- **`Link`** - Runtime links between method results and other methods
- **`Tag`** - Metadata tags for organizing methods
- **`ExternalDocumentation`** - Links to external documentation
- **`ErrorObject`** - Application-defined error specifications

### References and Extensions

- **`Reference`** - Internal and external references using JSON Schema $ref
- **`Extensions`** - Specification extensions with x- prefixed fields

## Validation

The crate provides validation helpers for:

- **Specification compliance** - OpenRPC 1.3.2 constraints represented by this crate
- **Unique constraints** - Method names, parameter names, error codes are unique
- **Type consistency** - Schemas and examples match their expected types
- **URL and email format validation** - Proper format checking for contact info
- **Parameter ordering** - Required parameters before optional ones
- **Reference validity** - Internal references point to valid components

```rust
use ras_openrpc_types::{Info, OpenRpc, validation::Validate};

let openrpc = OpenRpc::v1_3_2(
    Info::new("Validation Example API", "1.0.0"),
    Vec::new(),
);

// Validate returns detailed error information
match openrpc.validate() {
    Ok(()) => println!("Valid OpenRPC document"),
    Err(e) => eprintln!("Validation failed: {}", e),
}
```

## JSON Schema Integration

Schema objects model the JSON Schema Draft 7 shapes used by OpenRPC:

```rust
use ras_openrpc_types::Schema;

let user_schema = Schema::object()
    .with_property("id", Schema::string().with_format("uuid"))
    .with_property("name", Schema::string().with_min_length(1))
    .with_property("email", Schema::string().with_format("email"))
    .with_property("age", Schema::integer().with_minimum(0.0))
    .require_property("id")
    .require_property("name")
    .require_property("email");
```

## Builder Patterns

All major types support ergonomic builder patterns using the `bon` crate:

```rust
use ras_openrpc_types::{Method, ContentDescriptor, Schema, ParameterStructure};

let method = Method::builder()
    .name("createUser")
    .summary("Create a new user")
    .description("Creates a new user account")
    .params(vec![
        ContentDescriptor::new("userData", Schema::object())
            .with_description("User data")
            .required()
            .into()
    ])
    .result(
        ContentDescriptor::new("userId", Schema::string())
            .with_description("Created user ID")
            .into()
    )
    .param_structure(ParameterStructure::ByName)
    .build();
```

## Error Handling

Error types provide detailed information about validation failures:

```rust
use ras_openrpc_types::{OpenRpcError, OpenRpcResult};

fn validate_document(openrpc: &OpenRpc) -> OpenRpcResult<()> {
    openrpc.validate()
}

// Error types include:
// - ValidationError: Specification constraint violations
// - JsonError: JSON parsing/serialization errors  
// - ReferenceError: Invalid or unresolvable references
// - MissingField: Required fields not provided
// - InvalidField: Field values that don't meet constraints
```

## Features

### Optional Features

- **`json-schema`** - Enables additional JSON Schema functionality via `schemars`

```toml
[dependencies]
ras-openrpc-types = { version = "0.1.1", features = ["json-schema"] }
```

## Checks

```bash
cargo test -p ras-openrpc-types --locked
cargo clippy -p ras-openrpc-types --all-targets --all-features --locked -- -D warnings
```

## License

This project is licensed under either MIT or Apache-2.0. See
[LICENSE-MIT](../../../LICENSE-MIT) and [LICENSE-APACHE](../../../LICENSE-APACHE).

## Contributing

This crate is part of the Rust Agent Stack workspace. Please see the main repository for contributing guidelines.
