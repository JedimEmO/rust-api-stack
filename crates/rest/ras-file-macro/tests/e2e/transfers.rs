use super::*;

#[tokio::test]
async fn upload_and_download_round_trips_declared_multipart_fields() {
    let service = DemoImpl::new();
    let storage = service.storage.clone();
    let server = demo_server_arc(service);
    let mut client = demo_client(server);
    client.set_bearer_token(Some("user-token"));

    let payload = b"streamed file".to_vec();
    let metadata = UploadMetadata {
        title: "demo".to_string(),
    };

    let form = DemoUploadMultipart::new()
        .file_bytes(
            payload.clone(),
            "blob.bin",
            Some("application/octet-stream"),
        )
        .expect("file part")
        .metadata(&metadata)
        .expect("json part")
        .comment("hello");

    let uploaded: UploadResponse = client.upload(form).await.expect("upload succeeds");
    assert_eq!(uploaded.size, payload.len() as u64);
    assert_eq!(uploaded.title, "demo");
    assert_eq!(uploaded.comment.as_deref(), Some("hello"));

    assert_eq!(storage.lock().unwrap().len(), 1);

    let response = client
        .download_by_file_id(uploaded.file_id.clone())
        .await
        .expect("download succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"file-0.bin\"; filename*=UTF-8''file-0.bin"
    );
    let downloaded = response.bytes().await.expect("download body");
    assert_eq!(downloaded.as_ref(), payload.as_slice());
}

#[cfg(all(not(target_arch = "wasm32"), feature = "fs"))]
#[tokio::test]
async fn upload_streams_file_part_from_disk_round_trips() {
    use std::io::Write as _;

    let service = DemoImpl::new();
    let storage = service.storage.clone();
    let server = demo_server_arc(service);
    let mut client = demo_client(server);
    client.set_bearer_token(Some("user-token"));

    // Write a temp file that the generated streaming `file(path, ...)` method
    // (tokio::fs::File -> ReaderStream -> MultipartBuilder::stream_part) reads
    // from disk. This is the only test that drives the from-disk streaming path.
    let payload = b"streamed-from-disk file contents".to_vec();
    let mut temp = tempfile::NamedTempFile::new().expect("create temp file");
    temp.write_all(&payload).expect("write temp file");
    temp.flush().expect("flush temp file");
    let path = temp.path().to_path_buf();

    let metadata = UploadMetadata {
        title: "demo".to_string(),
    };

    let form = DemoUploadMultipart::new()
        .file(&path, Some("blob.bin"), Some("application/octet-stream"))
        .await
        .expect("streaming file part from disk")
        .metadata(&metadata)
        .expect("json part")
        .comment("hello");

    let uploaded: UploadResponse = client.upload(form).await.expect("upload succeeds");
    assert_eq!(uploaded.size, payload.len() as u64);
    assert_eq!(uploaded.title, "demo");
    assert_eq!(uploaded.comment.as_deref(), Some("hello"));

    assert_eq!(storage.lock().unwrap().len(), 1);

    // Verify the exact bytes survived the from-disk streaming multipart framing.
    let response = client
        .download_by_file_id(uploaded.file_id.clone())
        .await
        .expect("download succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let downloaded = response.bytes().await.expect("download body");
    assert_eq!(downloaded.as_ref(), payload.as_slice());

    drop(temp);
}

#[tokio::test]
async fn download_returns_not_found_for_missing_file() {
    let server = demo_server(DemoImpl::new());

    let response = server.get("/files/download/missing").await;

    response.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn optional_auth_download_without_token_is_anonymous() {
    let server = demo_server(DemoImpl::new());

    let response = server.get("/files/peek/x1").await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.text(), "anonymous:x1");
}

#[tokio::test]
async fn optional_auth_download_with_valid_token_sees_user() {
    let server = demo_server(DemoImpl::new());

    let response = server
        .get("/files/peek/x2")
        .authorization_bearer("user-token")
        .await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.text(), "user-1:x2");
}

#[tokio::test]
async fn optional_auth_download_with_invalid_token_is_lenient() {
    let server = demo_server(DemoImpl::new());

    let response = server
        .get("/files/peek/x3")
        .authorization_bearer("not-a-real-token")
        .await;

    // Lenient: a bad credential downgrades to anonymous rather than 401.
    response.assert_status(StatusCode::OK);
    assert_eq!(response.text(), "anonymous:x3");
}
