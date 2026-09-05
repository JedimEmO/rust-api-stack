use super::*;

#[tokio::test]
async fn auth_get_with_path_param_succeeds_with_user_token() {
    let response = server()
        .get("/api/items/7")
        .authorization_bearer("user-token")
        .await;
    response.assert_status_ok();
    let item: Item = response.json();

    assert_eq!(item.id, 7);
    assert_eq!(item.name, "item-7");
}

#[tokio::test]
async fn query_params_required_and_optional_serialize_correctly() {
    // Drive the generated client over the in-process transport so the
    // serde_urlencoded query path is exercised live (required + Option-skip).
    let client = client();

    let resp = client
        .get_search("hi".to_string(), Some(3), true)
        .await
        .expect("get_search with limit failed");
    assert_eq!(resp.items.len(), 3);
    assert_eq!(resp.items[0].name, "exact:hi-0");
    assert_eq!(resp.items[2].name, "exact:hi-2");

    // `limit: None` must be skipped from the query string entirely.
    let resp = client
        .get_search("zz".to_string(), None, false)
        .await
        .expect("get_search without limit failed");
    assert_eq!(resp.items.len(), 2);
    assert_eq!(resp.items[0].name, "fuzzy:zz-0");
}

#[tokio::test]
async fn vec_query_params_serialize_as_repeated_keys() {
    // `Vec<T>` and `Option<Vec<T>>` query params must serialize as repeated
    // keys through the generated client.
    let client = client();

    let resp = client
        .get_filter(
            vec!["red".to_string(), "blue".to_string()],
            Some(vec!["featured".to_string()]),
        )
        .await
        .expect("get_filter with tags failed");
    let names: Vec<_> = resp.items.into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["tag:red", "tag:blue", "optional:featured"]);

    let resp = client
        .get_filter(vec!["solo".to_string()], None)
        .await
        .expect("get_filter solo failed");
    let names: Vec<_> = resp.items.into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["tag:solo"]);
}

#[tokio::test]
async fn enum_query_params_use_serde_renames_without_display() {
    // Enum query values must honor `#[serde(rename)]` (asc/desc) rather than
    // any Display/Debug formatting.
    let client = client();

    let resp = client
        .get_sorted(SortOrder::Asc)
        .await
        .expect("get_sorted asc failed");
    assert_eq!(resp.items[0].name, "order:asc");

    let resp = client
        .get_sorted(SortOrder::Desc)
        .await
        .expect("get_sorted desc failed");
    assert_eq!(resp.items[0].name, "order:desc");
}

#[tokio::test]
async fn query_params_with_body_and_auth() {
    // Combined: bool query param + JSON body + bearer auth, via the client.
    let mut client = client();
    client.set_bearer_token(Some("admin-token"));

    let item = client
        .post_items_batch(
            true,
            CreateItem {
                name: "alpha".into(),
            },
        )
        .await
        .expect("post_items_batch notify=true failed");
    assert_eq!(item.name, "alpha(notified)");

    let item = client
        .post_items_batch(
            false,
            CreateItem {
                name: "beta".into(),
            },
        )
        .await
        .expect("post_items_batch notify=false failed");
    assert_eq!(item.name, "beta(silent)");
}

#[tokio::test]
async fn query_params_with_path_param() {
    // Path param substitution + Option query param + bearer auth, via client.
    let mut client = client();
    client.set_bearer_token(Some("user-token"));

    let resp = client
        .get_items_by_id_related(42, Some("featured".to_string()))
        .await
        .expect("get_items_by_id_related with tag failed");
    assert_eq!(resp.items[0].id, 42);
    assert_eq!(resp.items[0].name, "related/featured");

    let resp = client
        .get_items_by_id_related(42, None)
        .await
        .expect("get_items_by_id_related without tag failed");
    assert_eq!(resp.items[0].name, "related/none");
}
