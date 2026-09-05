use super::*;

#[tokio::test]
async fn upload_rejects_content_length_over_total_before_beginning_upload() {
    let service = DemoImpl::new();
    let begins = service.begins.clone();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .add_header("content-length", "4096")
        .content_type("multipart/form-data; boundary=x")
        .bytes(Bytes::new())
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*begins.lock().unwrap(), 0);
    assert_eq!(*aborts.lock().unwrap(), 0);
}

#[tokio::test]
async fn upload_rejects_duplicate_file_part_and_aborts_once() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = form("first").add_part(
        "file",
        Part::bytes("second")
            .file_name("second.bin")
            .mime_type("application/octet-stream"),
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
async fn upload_rejects_missing_required_filename_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let form = MultipartForm::new()
        .add_part(
            "file",
            Part::bytes("body").mime_type("application/octet-stream"),
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

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_file_over_part_limit_and_aborts() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = demo_server(service);

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form(vec![b'x'; 1025]))
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_text_over_part_limit_and_aborts() {
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
        .add_text("comment", "x".repeat(129));

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*aborts.lock().unwrap(), 1);
}

#[tokio::test]
async fn upload_rejects_missing_required_field() {
    let service = DemoImpl::new();
    let aborts = service.aborts.clone();
    let server = mock_http_server(
        DemoBuilder::<DemoImpl, MockAuthProvider>::new(service)
            .auth_provider(MockAuthProvider::default())
            .build(),
    );

    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes("body")
            .file_name("blob.bin")
            .mime_type("application/octet-stream"),
    );

    let response = server
        .post("/files/upload")
        .authorization_bearer("user-token")
        .multipart(form)
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(*aborts.lock().unwrap(), 1);
}
