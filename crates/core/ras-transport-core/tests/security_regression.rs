//! Security regression tests.
//!
//! Each test pins a concrete attack: it must fail before the corresponding fix
//! and pass after. See the security review of `feat/transport-trait-abstraction`.

use futures_util::StreamExt;
use http::Method;
use ras_transport_core::TransportRequest;
use ras_transport_core::multipart::MultipartBuilder;
use ras_transport_core::request::RequestBody;

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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A part `Content-Type` carrying CRLF must not be able to inject extra header
/// lines (or prematurely terminate the part header block) into the multipart
/// frame. `name`/`filename` are already escaped by `escape_disposition_param`;
/// `content_type` was written verbatim, so this is the gap.
#[tokio::test]
async fn content_type_crlf_cannot_inject_multipart_headers() {
    let malicious = "image/png\r\nX-Injected: evil\r\nContent-Type: text/html";
    let (body, _ct) = MultipartBuilder::with_boundary("BOUND")
        .bytes_part("file", "a.png", malicious, b"data".to_vec())
        .build();

    let bytes = collect_body(body).await;
    let wire = String::from_utf8_lossy(&bytes);

    // No injected header line may appear as its own CRLF-delimited header.
    assert!(
        !contains(&bytes, b"\r\nX-Injected: evil"),
        "CRLF injection in Content-Type produced a forged header line:\n{wire}"
    );
    // The header block must not be terminated early by an injected blank line
    // before the legitimate one (a single part has exactly one blank line that
    // ends its header block).
    assert_eq!(
        wire.matches("\r\n\r\n").count(),
        1,
        "Content-Type injection altered the header/body boundary:\n{wire}"
    );
}

/// A bearer token that cannot be encoded as a header value must NOT result in a
/// request that is silently sent without an `Authorization` header (fail-open).
/// `bearer()` is the dedicated auth helper, so it must fail closed.
#[test]
fn bearer_rejects_unencodable_token_instead_of_sending_unauthenticated() {
    // A control character cannot live in an HTTP header value.
    let result = TransportRequest::new(Method::GET, "https://api/x").bearer("tok\r\ninjected");
    assert!(
        matches!(
            result,
            Err(ras_transport_core::TransportError::InvalidHeader(_))
        ),
        "bearer() must fail closed on an unencodable token, got: {result:?}"
    );

    // A well-formed token still works and sets the header.
    let req = TransportRequest::new(Method::GET, "https://api/x")
        .bearer("good-token")
        .expect("valid token");
    assert_eq!(
        req.headers.get("authorization").unwrap(),
        "Bearer good-token"
    );
}
