use super::*;

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_sends_bearer_and_succeeds_with_permission() {
    let mut client = demo_client();
    client.set_bearer_token(Some("user-token"));

    let resp = client
        .add(AddRequest { a: 7, b: 35 })
        .await
        .expect("authenticated add should succeed");

    assert_eq!(resp.sum, 42);
}

#[cfg(feature = "client")]
#[tokio::test]
async fn generated_client_surfaces_jsonrpc_error_on_missing_permission() {
    let client = demo_client();

    let err = client
        .add(AddRequest { a: 1, b: 2 })
        .await
        .expect_err("anonymous add must be rejected as a JSON-RPC error");

    match err {
        ras_transport_core::TransportError::JsonRpc { message, .. } => {
            let m = message.to_lowercase();
            assert!(
                m.contains("auth") || m.contains("permission"),
                "expected auth/permission error, got: {message}"
            );
        }
        other => panic!("expected JsonRpc error variant, got: {other:?}"),
    }
}

#[tokio::test]
async fn unauth_method_round_trips() {
    let server = server();

    let resp: EchoResponse = call_rpc(
        &server,
        "ping",
        json!(EchoRequest {
            msg: "hello".to_string(),
        }),
        None,
    )
    .await
    .expect("ping ok");

    assert_eq!(resp.msg, "hello");
    assert_eq!(resp.user_id, None);
}

#[tokio::test]
async fn optional_auth_method_anonymous_without_token() {
    let server = server();

    let resp: EchoResponse = call_rpc(
        &server,
        "whoami",
        json!(EchoRequest {
            msg: "hi".to_string()
        }),
        None,
    )
    .await
    .expect("whoami ok for anonymous");

    assert_eq!(resp.msg, "hi");
    assert_eq!(resp.user_id, None);
}

#[tokio::test]
async fn optional_auth_method_identifies_valid_token() {
    let server = server();

    let resp: EchoResponse = call_rpc(
        &server,
        "whoami",
        json!(EchoRequest {
            msg: "hi".to_string()
        }),
        Some("user-token"),
    )
    .await
    .expect("whoami ok for authenticated");

    assert_eq!(resp.user_id.as_deref(), Some("user-1"));
}

#[tokio::test]
async fn optional_auth_method_is_lenient_with_bad_token() {
    let server = server();

    // A present-but-invalid token must NOT reject an OPTIONAL_AUTH method.
    let resp: EchoResponse = call_rpc(
        &server,
        "whoami",
        json!(EchoRequest {
            msg: "hi".to_string()
        }),
        Some("not-a-real-token"),
    )
    .await
    .expect("whoami stays lenient for a bad token");

    assert_eq!(resp.user_id, None);
}

#[tokio::test]
async fn permission_required_method_rejects_anonymous() {
    let server = server();

    let err = call_rpc::<AddResponse>(&server, "add", json!(AddRequest { a: 2, b: 3 }), None)
        .await
        .expect_err("anonymous add must be rejected");

    let s = err.to_string();
    assert!(
        s.contains("Authentication") || s.contains("AUTH") || s.contains("auth"),
        "expected auth-related error, got: {s}"
    );
}

#[tokio::test]
async fn permission_required_method_rejects_wrong_perms() {
    let server = server();

    let err = call_rpc::<AddResponse>(
        &server,
        "add",
        json!(AddRequest { a: 2, b: 3 }),
        Some("readonly-token"),
    )
    .await
    .expect_err("readonly user must not be allowed to call add");
    let s = err.to_string();
    assert!(
        s.contains("permission") || s.contains("Permission") || s.contains("PERMISSION"),
        "expected permission-related error, got: {s}"
    );
}

#[tokio::test]
async fn permission_required_method_succeeds_with_correct_perms() {
    let server = server();

    let resp: AddResponse = call_rpc(
        &server,
        "add",
        json!(AddRequest { a: 7, b: 35 }),
        Some("user-token"),
    )
    .await
    .expect("add ok");
    assert_eq!(resp.sum, 42);
}

#[tokio::test]
async fn admin_method_succeeds_with_admin_token() {
    let server = server();

    let resp: EchoResponse = call_rpc(
        &server,
        "admin_only",
        json!(EchoRequest {
            msg: "secret".to_string(),
        }),
        Some("admin-token"),
    )
    .await
    .expect("admin call ok");

    assert_eq!(resp.msg, "secret");
    assert_eq!(resp.user_id.as_deref(), Some("admin-1"));
}
