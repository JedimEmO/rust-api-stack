use super::*;

#[tokio::test]
async fn generated_file_client_timeout_variants_round_trip() {
    let service = DemoImpl::new();
    let storage = service.storage.clone();
    let server = demo_server_arc(service);
    let mut client = demo_client(server);
    client.set_bearer_token(Some("user-token"));

    let payload = b"timeout upload".to_vec();
    let metadata = UploadMetadata {
        title: "timeout".to_string(),
    };
    let form = DemoUploadMultipart::new()
        .file_bytes(
            payload.clone(),
            "timeout.bin",
            Some("application/octet-stream"),
        )
        .expect("file part")
        .metadata(&metadata)
        .expect("json part");

    let uploaded = client
        .upload_with_timeout(form, std::time::Duration::from_secs(1))
        .await
        .expect("upload_with_timeout succeeds");
    assert_eq!(uploaded.size, payload.len() as u64);
    assert_eq!(storage.lock().unwrap().len(), 1);

    let response = client
        .download_by_file_id_with_timeout(uploaded.file_id, std::time::Duration::from_secs(1))
        .await
        .expect("download_by_file_id_with_timeout succeeds");
    let downloaded = response.bytes().await.expect("download body");
    assert_eq!(downloaded.as_ref(), payload.as_slice());
}

#[test]
fn generated_client_multipart_builder_covers_declared_parts() {
    let metadata = UploadMetadata {
        title: "demo".to_string(),
    };

    let (body, content_type) = DemoUploadMultipart::new()
        .file_bytes(
            b"body".to_vec(),
            "blob.bin",
            Some("application/octet-stream"),
        )
        .expect("file part")
        .metadata(&metadata)
        .expect("json part")
        .comment("hello")
        .into_body();

    assert!(content_type.starts_with("multipart/form-data; boundary="));
    match body {
        ras_transport_core::RequestBody::Stream(_) => {}
        other => panic!("expected streaming multipart body, got {other:?}"),
    }
}

#[test]
fn generated_openapi_documents_v2_multipart_contract() {
    let doc = generate_demo_openapi();

    let upload = &doc["paths"]["/upload"]["post"];
    assert_eq!(
        upload["requestBody"]["content"]["multipart/form-data"]["schema"]["properties"]["file"]["format"],
        "binary"
    );
    assert_eq!(upload["x-ras-file"]["maxTotalBytes"], 2048);
    assert_eq!(upload["x-permissions"], serde_json::json!(["user"]));

    let download = &doc["paths"]["/download/{file_id}"]["get"];
    assert_eq!(
        download["responses"]["200"]["content"]["application/octet-stream"]["schema"]["$ref"],
        "#/components/schemas/BinaryFileResponse"
    );
    assert_eq!(download["x-ras-file"]["ranges"], true);

    // OPTIONAL_AUTH route advertises an optional security requirement.
    let peek = &doc["paths"]["/peek/{file_id}"]["get"];
    assert_eq!(
        peek["security"],
        serde_json::json!([{}, { "bearerAuth": [] }])
    );
}
