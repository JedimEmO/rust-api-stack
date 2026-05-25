//! Basic macro generation test
//!
//! This test focuses on ensuring the macro generates working server and client code
//! that can be compiled and basic types are created correctly.

use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleRequest {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleResponse {
    pub result: String,
}

// Generate service with basic functionality
jsonrpc_bidirectional_service!({
    service_name: SimpleService,

    client_to_server: [
        UNAUTHORIZED ping(String) -> String,
        WITH_PERMISSIONS(["user"]) echo(SimpleRequest) -> SimpleResponse,
    ],

    server_to_client: [
        notification(String),
    ],

    server_to_client_calls: [
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_exist() {
        // This test ensures the macro generates the expected types
        // If it compiles, the basic macro generation is working
        let request = SimpleRequest {
            message: "ping".to_string(),
        };
        let response = SimpleResponse {
            result: "pong".to_string(),
        };

        assert_eq!(request.message, "ping");
        assert_eq!(response.result, "pong");
    }

    #[cfg(feature = "server")]
    #[test]
    fn test_server_trait_exists() {
        // Test that we can reference the generated trait
        use std::marker::PhantomData;

        // This will only compile if the trait exists
        fn _check_trait_exists<T: SimpleServiceService>(_: PhantomData<T>) {}
    }

    #[cfg(feature = "server")]
    #[test]
    fn test_builder_exists() {
        // Test that we can reference the generated builder
        use std::marker::PhantomData;

        // This will only compile if the builder exists
        fn _check_builder_exists<T, A>(_: PhantomData<SimpleServiceBuilder<T, A>>)
        where
            T: SimpleServiceService,
            A: ras_auth_core::AuthProvider,
        {
        }
    }
}
