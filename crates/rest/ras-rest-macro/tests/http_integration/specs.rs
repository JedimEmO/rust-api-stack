use super::*;

#[tokio::test]
async fn test_docs_explorer_routes_generated() {
    let server = create_rest_test_server();

    let docs_response = server.get("/api/v1/docs").await;
    assert_eq!(docs_response.status_code().as_u16(), 200);

    let docs = docs_response.text();
    assert!(docs.contains("\"TestRestService\""));
    assert!(docs.contains("\"rest\""));
    assert!(docs.contains("/api/v1/docs/openapi.json"));
    assert!(docs.contains("id=\"bearer-token\""));
    assert!(docs.contains("id=\"saved-list\""));

    let spec_response = server.get("/api/v1/docs/openapi.json").await;
    assert_eq!(spec_response.status_code().as_u16(), 200);

    let spec: serde_json::Value = spec_response.json();
    assert_eq!(spec["info"]["title"], "TestRestService REST API");
    assert!(spec["paths"].is_object());
}

#[tokio::test]
async fn test_openapi_generation() {
    let _ = TestRestServiceBuilder::new(TestRestServiceImpl);

    let openapi_doc = generate_testrestservice_openapi();
    assert_eq!(openapi_doc["openapi"], "3.0.3");

    let get_users = &openapi_doc["paths"]["/users"]["get"];
    assert_eq!(get_users["summary"], "List users.");
    assert_eq!(
        get_users["description"],
        "List users.\n\nReturns all users visible to the caller."
    );

    let post_users = &openapi_doc["paths"]["/users"]["post"];
    assert_eq!(post_users["summary"], "Create a user.");
    assert_eq!(post_users["description"], "Create a user.");

    let health = &openapi_doc["paths"]["/health"]["get"];
    assert_eq!(health["summary"], "GET /health");
    assert_eq!(health["description"], "Handles GET requests to /health");
}
