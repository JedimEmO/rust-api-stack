use super::*;

#[tokio::test]
async fn test_openrpc_generation() {
    // Test that OpenRPC document is generated correctly
    let openrpc_doc = generate_testservice_openrpc();

    assert_eq!(openrpc_doc["openrpc"], "1.3.2");
    assert_eq!(openrpc_doc["info"]["title"], "TestService JSON-RPC API");

    let methods = openrpc_doc["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 11); // We have 11 methods defined

    // Check that unauthorized methods don't have authentication metadata
    let sign_in_method = methods.iter().find(|m| m["name"] == "sign_in").unwrap();
    assert!(sign_in_method.get("x-authentication").is_none());

    // Check that admin methods have correct permissions
    let delete_method = methods
        .iter()
        .find(|m| m["name"] == "delete_everything")
        .unwrap();
    assert_eq!(
        delete_method["x-authentication"]["required"].as_bool(),
        Some(true)
    );
    assert_eq!(delete_method["x-permissions"][0], "admin");

    // Check that methods with multiple permissions are correct
    let moderate_method = methods
        .iter()
        .find(|m| m["name"] == "moderate_content")
        .unwrap();
    let permissions = moderate_method["x-permissions"].as_array().unwrap();
    assert_eq!(permissions.len(), 2);
    assert!(permissions.contains(&json!("admin")));
    assert!(permissions.contains(&json!("moderator")));
}
