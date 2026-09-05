use super::*;

#[cfg(feature = "client")]
#[test]
fn versioned_client_method_names_sanitize_semver_labels() {
    let _method = DemoClient::rename_user_v1_0_0;
    let _method_with_timeout = DemoClient::rename_user_v1_0_0_with_timeout;
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_round_trips_versioned_wire_method() {
    let client = demo_client();

    let resp = client
        .rename_user_v1_0_0(RenameUserV1 {
            name: "Ada".to_string(),
        })
        .await
        .expect("legacy versioned method should round-trip via client");

    assert_eq!(
        resp,
        RenameUserResponseV1 {
            name: "Ada".to_string()
        }
    );
}

#[tokio::test]
async fn legacy_version_round_trips_through_canonical_handler() {
    let server = server();

    let resp: RenameUserResponseV1 = call_rpc(
        &server,
        "rename_user.v1",
        json!(RenameUserV1 {
            name: "Ada".to_string(),
        }),
        None,
    )
    .await
    .expect("legacy rename ok");

    assert_eq!(
        resp,
        RenameUserResponseV1 {
            name: "Ada".to_string()
        }
    );
}

#[tokio::test]
async fn canonical_version_uses_declared_wire_method() {
    let server = server();

    let resp: RenameUserResponseV2 = call_rpc(
        &server,
        "rename_user.v2",
        json!(RenameUserV2 {
            display_name: "Grace".to_string(),
            notify: true,
        }),
        None,
    )
    .await
    .expect("canonical rename ok");

    assert_eq!(
        resp,
        RenameUserResponseV2 {
            display_name: "Grace".to_string(),
            notified: true,
        }
    );
}
