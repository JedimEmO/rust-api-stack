//! OpenRPC Types
//!
//! Rust types for the OpenRPC 1.3.2 specification with serde support,
//! bon builders, and runtime validation helpers.
//!
//! This crate provides:
//! - OpenRPC 1.3.2 specification types
//! - Serde serialization/deserialization support
//! - Bon builder patterns for ergonomic API construction
//! - Validation helpers for OpenRPC constraints
//! - JSON Schema Draft 7 compatibility for Schema objects
//! - Reference resolution support for $ref within components
//! - Specification extensions support for x-* extension fields
//!
//! # Example
//!
//! ```rust
//! use openrpc_types::{OpenRpc, Info, Method, ContentDescriptor, Schema, MethodOrReference, ContentDescriptorOrReference, ContentDescriptorSchema};
//!
//! let openrpc = OpenRpc::builder()
//!     .openrpc("1.3.2".to_string())
//!     .info(
//!         Info::builder()
//!             .title("Example API".to_string())
//!             .version("1.0.0".to_string())
//!             .build()
//!     )
//!     .methods(vec![
//!         MethodOrReference::Method(Box::new(Method::builder()
//!             .name("hello".to_string())
//!             .params(vec![])
//!             .result(ContentDescriptorOrReference::ContentDescriptor(Box::new(
//!                 ContentDescriptor::builder()
//!                     .name("greeting".to_string())
//!                     .schema(ContentDescriptorSchema::Schema(Box::new(Schema::string())))
//!                     .build()
//!             )))
//!             .build()))
//!     ])
//!     .build();
//! ```

pub mod error;
pub mod validation;

// Core OpenRPC specification types
mod components;
mod content_descriptor;
mod error_object;
mod example;
mod extensions;
mod external_docs;
mod info;
mod link;
mod method;
mod openrpc;
mod reference;
mod schema;
mod server;
mod tag;

// Re-export all public types
pub use components::*;
pub use content_descriptor::*;
pub use error_object::*;
pub use example::*;
pub use extensions::*;
pub use external_docs::*;
pub use info::*;
pub use link::*;
pub use method::*;
pub use openrpc::*;
pub use reference::*;
pub use schema::*;
pub use server::*;
pub use tag::*;

pub use error::*;
pub use validation::*;

/// OpenRPC specification version constants
pub mod version {
    /// Current OpenRPC specification version (1.3.2)
    pub const CURRENT: &str = "1.3.2";

    /// All supported OpenRPC specification versions
    pub const SUPPORTED: &[&str] = &["1.0.0", "1.1.0", "1.2.0", "1.3.0", "1.3.1", "1.3.2"];

    /// Check if a version string is supported
    pub fn is_supported(version: &str) -> bool {
        SUPPORTED.contains(&version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert!(version::is_supported("1.3.2"));
        assert!(version::is_supported("1.0.0"));
        assert!(!version::is_supported("2.0.0"));
        assert!(!version::is_supported("0.9.0"));
    }
}
