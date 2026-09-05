use super::*;

#[tokio::test]
async fn unauth_get_round_trips() {
    let response = server().get("/api/items").await;
    response.assert_status_ok();
    let resp: ItemsResponse = response.json();

    assert_eq!(resp.items.len(), 1);
    assert_eq!(resp.items[0].name, "alpha");
}

#[tokio::test]
async fn auth_get_rejected_without_token() {
    let response = server().get("/api/items/1").await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_post_rejected_with_insufficient_perms() {
    let response = server()
        .post("/api/items")
        .authorization_bearer("user-token")
        .json(&CreateItem {
            name: "x".to_string(),
        })
        .await;
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn auth_post_with_admin_succeeds_and_user_id_propagates() {
    let response = server()
        .post("/api/items")
        .authorization_bearer("admin-token")
        .json(&CreateItem { name: "foo".into() })
        .await;
    response.assert_status(StatusCode::CREATED);
    let item: Item = response.json();

    assert_eq!(item.name, "foo");
    // admin-1 is 7 chars long.
    assert_eq!(item.id, 7);
}

#[tokio::test]
async fn optional_auth_without_token_sees_anonymous_caller() {
    let response = server().get("/api/whoami").await;
    response.assert_status_ok();
    let resp: WhoamiResponse = response.json();
    assert_eq!(resp.caller, "anonymous");
}

#[tokio::test]
async fn optional_auth_with_valid_token_sees_authenticated_caller() {
    let response = server()
        .get("/api/whoami")
        .authorization_bearer("user-token")
        .await;
    response.assert_status_ok();
    let resp: WhoamiResponse = response.json();
    assert_eq!(resp.caller, "user-1");
}

#[tokio::test]
async fn optional_auth_with_invalid_token_is_lenient_and_anonymous() {
    // A present-but-bad credential must NOT reject an OPTIONAL_AUTH route; it
    // downgrades to anonymous.
    let response = server()
        .get("/api/whoami")
        .authorization_bearer("not-a-real-token")
        .await;
    response.assert_status_ok();
    let resp: WhoamiResponse = response.json();
    assert_eq!(resp.caller, "anonymous");
}

#[tokio::test]
async fn optional_auth_post_threads_caller_and_body() {
    // Anonymous POST with a body still reaches the handler.
    let anon = server()
        .post("/api/whoami/echo")
        .json(&CreateItem { name: "hi".into() })
        .await;
    anon.assert_status_ok();
    assert_eq!(anon.json::<WhoamiResponse>().caller, "anonymous:hi");

    // Authenticated POST sees the caller and the body.
    let authed = server()
        .post("/api/whoami/echo")
        .authorization_bearer("user-token")
        .json(&CreateItem { name: "hi".into() })
        .await;
    authed.assert_status_ok();
    assert_eq!(authed.json::<WhoamiResponse>().caller, "user-1:hi");
}
