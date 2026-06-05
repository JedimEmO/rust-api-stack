//! Coverage for the streaming multipart paths (`stream_part`, `file_path`) and
//! `Content-Disposition` escaping. Native + `fs` feature only. Uses a temp file
//! on disk (no network/sockets).

#![cfg(all(not(target_arch = "wasm32"), feature = "fs"))]

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use ras_transport_core::multipart::MultipartBuilder;
use ras_transport_core::request::RequestBody;
use ras_transport_core::{ByteStream, TransportError, byte_stream_from};

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
async fn stream_part_produces_exact_framing_from_multiple_chunks() {
    // A multi-chunk stream must be framed as one part with all chunks contiguous.
    let chunks: Vec<&'static [u8]> = vec![b"abc", b"def", b"ghi"];
    let stream: ByteStream = byte_stream_from(stream::iter(
        chunks
            .into_iter()
            .map(|c| Ok::<Bytes, TransportError>(Bytes::from_static(c))),
    ));

    let (body, content_type) = MultipartBuilder::with_boundary("BOUND")
        .stream_part("upload", "data.bin", "application/octet-stream", stream)
        .build();

    assert_eq!(content_type, "multipart/form-data; boundary=BOUND");
    let bytes = collect_body(body).await;
    let expected = concat!(
        "--BOUND\r\n",
        "Content-Disposition: form-data; name=\"upload\"; filename=\"data.bin\"\r\n",
        "Content-Type: application/octet-stream\r\n",
        "\r\n",
        "abcdefghi\r\n",
        "--BOUND--\r\n",
    );
    assert_eq!(String::from_utf8(bytes).unwrap(), expected);
}

#[tokio::test]
async fn file_path_streams_disk_contents_into_a_part() {
    // Write a temp file, then stream it through file_path (tokio::fs -> ReaderStream).
    let dir = std::env::temp_dir();
    let path = dir.join(format!("ras-transport-fp-{}.txt", std::process::id()));
    tokio::fs::write(&path, b"file-contents-here")
        .await
        .expect("write temp file");

    let (body, _ct) = MultipartBuilder::with_boundary("B")
        .file_path("doc", None, "text/plain", &path)
        .await
        .expect("file_path")
        .build();

    let bytes = collect_body(body).await;
    let text = String::from_utf8(bytes).unwrap();
    // filename is derived from the path's file name.
    let fname = path.file_name().unwrap().to_string_lossy();
    assert!(text.contains(&format!(
        "Content-Disposition: form-data; name=\"doc\"; filename=\"{fname}\""
    )));
    assert!(text.contains("Content-Type: text/plain\r\n"));
    assert!(text.contains("file-contents-here"));
    assert!(text.ends_with("--B--\r\n"));

    tokio::fs::remove_file(&path).await.ok();
}

#[tokio::test]
async fn disposition_params_with_quotes_and_newlines_are_escaped() {
    // A hostile filename must not break the frame: " -> %22, CR/LF -> %0D/%0A.
    let (body, _ct) = MultipartBuilder::with_boundary("B")
        .bytes_part(
            "field",
            "ev\"il\r\n.txt",
            "application/octet-stream",
            Bytes::from_static(b"x"),
        )
        .build();
    let text = String::from_utf8(collect_body(body).await).unwrap();
    assert!(text.contains("filename=\"ev%22il%0D%0A.txt\""));
    // The header line still terminates cleanly and the single part frames correctly.
    assert!(text.ends_with("x\r\n--B--\r\n"));
}
