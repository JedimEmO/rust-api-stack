use super::*;

#[tokio::test]
async fn generated_client_timeout_variant_accepts_duration() {
    let client = client();

    let resp = client
        .get_search_with_timeout(
            "timeout".to_string(),
            Some(1),
            false,
            std::time::Duration::from_secs(1),
        )
        .await
        .expect("get_search_with_timeout failed");

    assert_eq!(resp.items.len(), 1);
    assert_eq!(resp.items[0].name, "fuzzy:timeout-0");
}

#[tokio::test]
async fn handler_error_surfaces_to_client() {
    let response = server()
        .get("/api/items/404")
        .authorization_bearer("user-token")
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}
