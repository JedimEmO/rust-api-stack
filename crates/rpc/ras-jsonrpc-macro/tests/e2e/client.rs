use super::*;

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_round_trips_over_axum_transport() {
    let client = demo_client();

    let resp = client
        .ping(EchoRequest {
            msg: "hello-from-client".to_string(),
        })
        .await
        .expect("ping over transport should succeed");

    assert_eq!(resp.msg, "hello-from-client");
    assert_eq!(resp.user_id, None);
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_timeout_variant_accepts_duration() {
    let client = demo_client();

    let resp = client
        .ping_with_timeout(
            EchoRequest {
                msg: "timeout-client".to_string(),
            },
            std::time::Duration::from_secs(1),
        )
        .await
        .expect("ping_with_timeout over transport should succeed");

    assert_eq!(resp.msg, "timeout-client");
    assert_eq!(resp.user_id, None);
}
