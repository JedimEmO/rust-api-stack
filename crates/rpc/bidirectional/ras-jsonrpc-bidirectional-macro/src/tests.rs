//! Tests for the bidirectional macro generation

use crate::{AuthRequirement, BidirectionalServiceDefinition, generate_service_code};

#[test]
fn test_macro_compiles() {
    // This is a basic compilation test to ensure the macro expands without syntax errors
    let input = r#"{
            service_name: TestService,
            client_to_server: [
                UNAUTHORIZED test_method(String) -> String,
                WITH_PERMISSIONS(["admin"]) admin_method(u32) -> bool,
            ],
            server_to_client: [
                user_notification(String),
                status_update(u32),
            ],
            server_to_client_calls: [
            ]
        }"#;

    // Parse the macro input
    let parsed: BidirectionalServiceDefinition = syn::parse_str(input).unwrap();

    // Verify parsing worked correctly
    assert_eq!(parsed.service_name.to_string(), "TestService");
    assert_eq!(parsed.client_to_server.len(), 2);
    assert_eq!(parsed.server_to_client.len(), 2);

    // Generate code
    let generated = generate_service_code(parsed);
    assert!(generated.is_ok());
}

#[test]
fn test_simple_parsing() {
    let input = r#"{
            service_name: SimpleService,
            client_to_server: [
                UNAUTHORIZED hello(String) -> String,
            ],
            server_to_client: [
                notification(u32),
            ],
            server_to_client_calls: [
            ]
        }"#;

    let parsed: BidirectionalServiceDefinition = syn::parse_str(input).unwrap();
    assert_eq!(parsed.service_name.to_string(), "SimpleService");
    assert_eq!(parsed.client_to_server.len(), 1);
    assert_eq!(parsed.server_to_client.len(), 1);
}

#[test]
fn test_permission_parsing() {
    let input = r#"{
            service_name: PermissionService,
            client_to_server: [
                WITH_PERMISSIONS(["admin", "write"] | ["super_admin"]) complex_method(String) -> String,
            ],
            server_to_client: [],
            server_to_client_calls: [
            ]
        }"#;

    let parsed: BidirectionalServiceDefinition = syn::parse_str(input).unwrap();

    if let AuthRequirement::WithPermissions(groups) = &parsed.client_to_server[0].auth {
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], vec!["admin", "write"]);
        assert_eq!(groups[1], vec!["super_admin"]);
    } else {
        panic!("Expected WithPermissions auth requirement");
    }
}

#[test]
fn test_server_to_client_calls_parse_without_auth_prefix() {
    let input = r#"{
            service_name: CallbackService,
            client_to_server: [],
            server_to_client: [],
            server_to_client_calls: [
                get_status(String) -> bool,
            ]
        }"#;

    let parsed: BidirectionalServiceDefinition = syn::parse_str(input).unwrap();

    assert_eq!(parsed.server_to_client_calls.len(), 1);
    assert_eq!(
        parsed.server_to_client_calls[0].name.to_string(),
        "get_status"
    );
    assert!(matches!(
        parsed.server_to_client_calls[0].auth,
        AuthRequirement::Unauthorized
    ));
}
