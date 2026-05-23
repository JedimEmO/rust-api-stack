//! Macro compilation tests for bidirectional JSON-RPC
//!
//! Tests that the macro generates valid code for various scenarios

use ras_jsonrpc_bidirectional_macro::jsonrpc_bidirectional_service;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub text: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub message_id: u64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserJoinedNotification {
    pub username: String,
    pub user_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBroadcast {
    pub message: ChatMessage,
    pub message_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemNotification {
    pub message: String,
    pub level: String,
}

// Test complex service with multiple authentication levels
jsonrpc_bidirectional_service!({
    service_name: ChatService,

    client_to_server: [
        UNAUTHORIZED join_chat(String) -> String,
        WITH_PERMISSIONS(["user"]) send_message(ChatMessage) -> ChatResponse,
        WITH_PERMISSIONS(["admin"]) broadcast_system_message(String) -> (),
    ],

    server_to_client: [
        user_joined(UserJoinedNotification),
        message_received(MessageBroadcast),
        system_notification(SystemNotification),
    ],

    server_to_client_calls: [
    ]
});

// Test simple service
jsonrpc_bidirectional_service!({
    service_name: EchoService,

    client_to_server: [
        UNAUTHORIZED echo(String) -> String,
    ],

    server_to_client: [
        notification(String),
    ],

    server_to_client_calls: [
    ]
});

// Test documented server-to-client calls syntax without auth prefixes
jsonrpc_bidirectional_service!({
    service_name: CallbackService,

    client_to_server: [
    ],

    server_to_client: [
    ],

    server_to_client_calls: [
        request_status(String) -> bool,
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macro_generates_valid_code() {
        // The fact that this compiles proves the macro works
        let request = ChatMessage {
            text: "hello".to_string(),
            username: "alice".to_string(),
        };

        assert_eq!(request.text, "hello");
    }

    #[cfg(feature = "server")]
    #[test]
    fn test_server_traits_exist() {
        // Test that generated server traits exist
        use std::marker::PhantomData;

        fn _check_chat_service<T: ChatServiceService>(_: PhantomData<T>) {}
        fn _check_echo_service<T: EchoServiceService>(_: PhantomData<T>) {}
        fn _check_callback_service<T: CallbackServiceService>(_: PhantomData<T>) {}
    }

    #[cfg(feature = "server")]
    #[test]
    fn test_builders_exist() {
        // Test that generated builders exist
        use std::marker::PhantomData;

        fn _check_chat_builder<T, A>(_: PhantomData<ChatServiceBuilder<T, A>>)
        where
            T: ChatServiceService,
            A: ras_auth_core::AuthProvider,
        {
        }

        fn _check_echo_builder<T, A>(_: PhantomData<EchoServiceBuilder<T, A>>)
        where
            T: EchoServiceService,
            A: ras_auth_core::AuthProvider,
        {
        }

        fn _check_callback_builder<T, A>(_: PhantomData<CallbackServiceBuilder<T, A>>)
        where
            T: CallbackServiceService,
            A: ras_auth_core::AuthProvider,
        {
        }
    }

    #[test]
    fn test_data_types_serialize() {
        // Test that all data types can be serialized/deserialized
        let chat_message = ChatMessage {
            text: "Hello world!".to_string(),
            username: "alice".to_string(),
        };

        let json = serde_json::to_string(&chat_message).unwrap();
        let deserialized: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.text, "Hello world!");
        assert_eq!(deserialized.username, "alice");

        let response = ChatResponse {
            message_id: 42,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: ChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_id, 42);
        assert_eq!(deserialized.timestamp, "2024-01-01T00:00:00Z");
    }
}
