use super::*;

#[test]
fn jsonrpc_error_from_server_error_sends_generic_message_not_handler_detail() {
    // Handler error carrying a secret -> client sees only a generic message,
    // stable code preserved, no data field.
    let err = ServerError::Internal("database password is hunter2".into());
    let jsonrpc = jsonrpc_error_from_server_error(&err);
    assert_eq!(jsonrpc.code, error_codes::INTERNAL_ERROR);
    assert_eq!(jsonrpc.message, "Internal error");
    assert!(!jsonrpc.message.contains("hunter2"));
    assert!(jsonrpc.data.is_none());

    // AuthError detail must not reach the client either.
    let auth = ServerError::AuthenticationFailed(ras_auth_core::AuthError::Internal(
        "dsn=postgres://user:pw@host/db".into(),
    ));
    let jsonrpc = jsonrpc_error_from_server_error(&auth);
    assert_eq!(jsonrpc.code, error_codes::AUTHENTICATION_REQUIRED);
    assert_eq!(jsonrpc.message, "Authentication failed");
    assert!(!jsonrpc.message.contains("dsn"));

    // Stable codes for the invalid-request / method-not-found classes.
    assert_eq!(
        jsonrpc_error_from_server_error(&ServerError::InvalidRequest("Invalid params: x".into()))
            .code,
        error_codes::INVALID_REQUEST
    );
    assert_eq!(
        jsonrpc_error_from_server_error(&ServerError::HandlerNotFound("m".into())).code,
        error_codes::METHOD_NOT_FOUND
    );
}

#[tokio::test]
async fn handler_loop_sends_jsonrpc_error_and_continues_without_socket() {
    let fail = JsonRpcRequest::new(
        "fail".into(),
        Some(serde_json::json!({})),
        Some(serde_json::json!(1)),
    );
    let ok = JsonRpcRequest::new(
        "ok".into(),
        Some(serde_json::json!({})),
        Some(serde_json::json!(2)),
    );
    let mut socket = InMemorySocket::closing([
        WebSocketIoMessage::Text(
            serde_json::to_string(&BidirectionalMessage::Request(fail)).unwrap(),
        ),
        WebSocketIoMessage::Text(
            serde_json::to_string(&BidirectionalMessage::Request(ok)).unwrap(),
        ),
    ]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(RecoveringHandler), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));

    let error_response = match &messages[1] {
        BidirectionalMessage::Response(response) => response,
        other => panic!("expected error response, got {other:?}"),
    };
    assert_eq!(error_response.id, Some(serde_json::json!(1)));
    let error = error_response.error.as_ref().expect("JSON-RPC error");
    assert_eq!(error.code, ras_jsonrpc_types::error_codes::INVALID_REQUEST);
    // Message is the generic per-class string; the handler's detail
    // ("bad request") stays server-side.
    assert_eq!(error.message, "Invalid request");

    let success_response = match &messages[2] {
        BidirectionalMessage::Response(response) => response,
        other => panic!("expected success response, got {other:?}"),
    };
    assert_eq!(success_response.id, Some(serde_json::json!(2)));
    assert_eq!(success_response.result.as_ref().unwrap()["method"], "ok");

    assert!(matches!(
        messages[3],
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}

#[tokio::test]
async fn handler_loop_answers_malformed_text_with_parse_error_and_continues() {
    let request = JsonRpcRequest::new(
        "echo".into(),
        Some(serde_json::json!({})),
        Some(serde_json::json!(9)),
    );
    let mut socket = InMemorySocket::closing([
        WebSocketIoMessage::Text("not json-rpc".to_string()),
        WebSocketIoMessage::Text(
            serde_json::to_string(&BidirectionalMessage::Request(request)).unwrap(),
        ),
    ]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(RespondingHandler), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));

    // The garbage frame is answered with -32700 (id null)...
    let parse_error = match &messages[1] {
        BidirectionalMessage::Response(response) => response,
        other => panic!("expected parse error response, got {other:?}"),
    };
    assert_eq!(parse_error.id, None);
    let error = parse_error.error.as_ref().expect("parse error");
    assert_eq!(error.code, ras_jsonrpc_types::error_codes::PARSE_ERROR);

    // ...and the connection keeps serving subsequent requests.
    let response = match &messages[2] {
        BidirectionalMessage::Response(response) => response,
        other => panic!("expected response, got {other:?}"),
    };
    assert_eq!(response.id, Some(serde_json::json!(9)));

    assert!(matches!(
        messages[3],
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}

#[tokio::test]
async fn handler_loop_closes_oversized_text_without_response() {
    let mut socket = InMemorySocket::closing([WebSocketIoMessage::Text("too large".into())]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 4)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert_eq!(messages.len(), 2);
    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));
    assert!(matches!(
        messages[1],
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}

#[tokio::test]
async fn handler_loop_ignores_non_utf8_binary_without_response() {
    let mut socket = InMemorySocket::closing([WebSocketIoMessage::Binary(vec![0xff, 0xfe])]);
    let (_tx, rx) = mpsc::channel(4);

    WebSocketHandler::new(Arc::new(PassThrough), ctx(), rx, 1024)
        .run_with_io(&mut socket)
        .await
        .unwrap();

    let messages = bidirectional_outgoing(&socket);
    assert_eq!(messages.len(), 2);
    assert!(matches!(
        messages[0],
        BidirectionalMessage::ConnectionEstablished { .. }
    ));
    assert!(matches!(
        messages[1],
        BidirectionalMessage::ConnectionClosed { .. }
    ));
}
