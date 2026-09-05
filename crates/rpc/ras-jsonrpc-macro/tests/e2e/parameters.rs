use super::*;

#[tokio::test]
async fn malformed_params_yield_jsonrpc_error() {
    // Bypass the typed client to send a malformed body and confirm the
    // server returns a JSON-RPC `invalid_params` error rather than a panic.
    let server = server();

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "params": { "bogus": 1 },
        "id": 1,
    });

    let resp: serde_json::Value = server.post("/rpc").json(&body).await.json();

    assert!(
        resp.get("error").is_some(),
        "expected error in response: {resp}"
    );
    let code = resp["error"]["code"].as_i64().unwrap();
    assert_eq!(code, -32602, "expected invalid_params (-32602), got {code}");
}
