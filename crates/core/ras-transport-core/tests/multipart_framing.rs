//! Multipart framing must produce exact RFC 7578 wire bytes.

use futures_util::StreamExt;
use ras_transport_core::request::RequestBody;
use ras_transport_core::multipart::MultipartBuilder;

async fn collect_body(body: RequestBody) -> Vec<u8> {
    match body {
        RequestBody::Stream(mut s) => {
            let mut out = Vec::new();
            while let Some(chunk) = s.next().await {
                out.extend_from_slice(&chunk.expect("chunk"));
            }
            out
        }
        RequestBody::Bytes(b) => b.to_vec(),
        RequestBody::Empty => Vec::new(),
    }
}

#[tokio::test]
async fn new_generates_a_boundary_and_default_matches() {
    // Covers new() / Default / generate_boundary (other tests use with_boundary).
    let builder = MultipartBuilder::default().text("field", "value");
    let ct = builder.content_type();
    assert!(
        ct.starts_with("multipart/form-data; boundary=ras-boundary-"),
        "unexpected content-type: {ct}"
    );
    let boundary = ct
        .strip_prefix("multipart/form-data; boundary=")
        .unwrap()
        .to_string();
    let (body, ct2) = builder.build();
    assert_eq!(ct, ct2);
    let text = String::from_utf8(collect_body(body).await).unwrap();
    assert!(text.starts_with(&format!("--{boundary}\r\n")));
    assert!(text.ends_with(&format!("--{boundary}--\r\n")));
}

#[tokio::test]
async fn text_json_bytes_combo_produces_exact_wire_bytes() {
    #[derive(serde::Serialize)]
    struct Meta {
        id: u32,
    }

    let builder = MultipartBuilder::with_boundary("BOUND")
        .text("field1", "hello")
        .json("meta", &Meta { id: 7 })
        .expect("json part")
        .bytes_part("file", "a.bin", "application/octet-stream", b"\x00\x01\x02".to_vec());

    let content_type = builder.content_type();
    assert_eq!(content_type, "multipart/form-data; boundary=BOUND");

    let (body, ct) = builder.build();
    assert_eq!(ct, "multipart/form-data; boundary=BOUND");

    let bytes = collect_body(body).await;

    let mut expected: Vec<u8> = Vec::new();
    expected.extend_from_slice(b"--BOUND\r\n");
    expected.extend_from_slice(b"Content-Disposition: form-data; name=\"field1\"\r\n");
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"hello");
    expected.extend_from_slice(b"\r\n");

    expected.extend_from_slice(b"--BOUND\r\n");
    expected.extend_from_slice(b"Content-Disposition: form-data; name=\"meta\"\r\n");
    expected.extend_from_slice(b"Content-Type: application/json\r\n");
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"{\"id\":7}");
    expected.extend_from_slice(b"\r\n");

    expected.extend_from_slice(b"--BOUND\r\n");
    expected.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"a.bin\"\r\n",
    );
    expected.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
    expected.extend_from_slice(b"\r\n");
    expected.extend_from_slice(b"\x00\x01\x02");
    expected.extend_from_slice(b"\r\n");

    expected.extend_from_slice(b"--BOUND--\r\n");

    assert_eq!(
        bytes,
        expected,
        "got:\n{}",
        String::from_utf8_lossy(&bytes)
    );
}
