use super::*;

#[tokio::test]
async fn upload_rejects_auth_before_beginning_upload() {
    let service = DemoImpl::new();
    let begins = service.begins.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let response = server.post("/files/upload").multipart(form("body")).await;

    response.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(*begins.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_request_content_type_before_beginning_upload() {
    let service = DemoImpl::new();
    let begins = service.begins.clone();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .text("not multipart")
        .content_type("text/plain")
        .await;

    response.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(*begins.lock().unwrap(), 0);
    assert_eq!(*aborts.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_unsupported_file_content_type_after_begin_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.txt")
                .mime_type("text/plain"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_wrong_json_content_type_after_begin_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("text/plain"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_unknown_field_when_configured_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form("body").add_text("extra", "ignored?"))
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_invalid_json_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text("{invalid").mime_type("application/json"),
        );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_invalid_utf8_text_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body")
                .file_name("blob.bin")
                .mime_type("application/octet-stream"),
        )
        .add_part(
            "metadata",
            Part::text(r#"{"title":"demo"}"#).mime_type("application/json"),
        )
        .add_part("comment", Part::bytes(vec![0xff, 0xfe]));

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_when_handler_does_not_consume_file_stream() {
    let service = DemoImpl::new().without_file_consumption();
    let aborts = service.aborts.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form("body"))
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}
