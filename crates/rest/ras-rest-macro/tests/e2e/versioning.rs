use super::*;

#[tokio::test]
async fn legacy_rest_version_round_trips_through_canonical_handler() {
    let response = server()
        .post("/api/v1/items/7/rename?notify=true")
        .json(&RenameItemV1 {
            name: "renamed".to_string(),
        })
        .await;
    response.assert_status_ok();
    let resp: RenamedItemV1 = response.json();

    assert_eq!(
        resp,
        RenamedItemV1 {
            name: "renamed".to_string()
        }
    );
}

#[tokio::test]
async fn optional_auth_versioned_legacy_path_threads_caller() {
    // v1 (legacy) path with a valid token: exercises the legacy/migration arm AND
    // caller resolution. The migrated v1 response carries display_name only.
    let response = server()
        .post("/api/v1/items/7/touch?notify=true")
        .authorization_bearer("user-token")
        .json(&RenameItemV1 {
            name: "hello".to_string(),
        })
        .await;
    response.assert_status_ok();
    let resp: RenamedItemV1 = response.json();
    // RenamedItemV1.name == migrated display_name == "<caller>:<input>".
    assert_eq!(resp.name, "user-1:hello");
}

#[tokio::test]
async fn optional_auth_versioned_canonical_path_is_anonymous_without_token() {
    let response = server()
        .post("/api/v2/items/9/touch?notify=false")
        .json(&RenameItemV2 {
            display_name: "world".to_string(),
            notify: false,
        })
        .await;
    response.assert_status_ok();
    let resp: RenamedItemV2 = response.json();
    assert_eq!(resp.id, 9);
    assert_eq!(resp.display_name, "anonymous:world");
}

#[tokio::test]
async fn canonical_rest_version_uses_v2_path_and_types() {
    let response = server()
        .post("/api/v2/items/8/rename?notify=false")
        .json(&RenameItemV2 {
            display_name: "canonical".to_string(),
            notify: true,
        })
        .await;
    response.assert_status_ok();
    let resp: RenamedItemV2 = response.json();

    assert_eq!(
        resp,
        RenamedItemV2 {
            id: 8,
            display_name: "canonical".to_string(),
            notified: true,
        }
    );
}
