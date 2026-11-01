//! Security regression tests.
//!
//! Each test pins a concrete attack: it must fail before the corresponding fix
//! and pass after. See the security review of `feat/transport-trait-abstraction`.

use bytes::Bytes;
use futures_util::StreamExt;
use http::Method;
use ras_transport_core::multipart::MultipartBuilder;
use ras_transport_core::request::RequestBody;
use ras_transport_core::{TransportError, TransportRequest, TransportResponse, byte_stream_from};

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
        matches!(result, Err(TransportError::InvalidHeader(_))),
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

/// URL parsers resolve `.` and `..` (and their `%2E` forms) as dot segments,
/// so `/items/{id}/delete` with an `id` of `..` would send the caller's
/// credentials to `/delete`. The encoder must refuse such values, and the
/// empty one, which leaves a `//` that some proxies merge.
#[test]
fn dot_only_and_empty_path_parameters_are_rejected() {
    for value in ["", ".", ".."] {
        let result = ras_transport_core::encode_path_segment(value);
        assert!(
            matches!(result, Err(TransportError::InvalidPathSegment(_))),
            "{value:?} must be rejected, got {result:?}"
        );
    }
}

/// Values that contain dots but are not dot segments keep their slot after
/// the URL is parsed.
#[cfg(feature = "reqwest")]
#[test]
fn values_with_dots_that_are_not_dot_segments_keep_the_endpoint_path() {
    for value in ["...", ".a", "a.", "a..b", "../x", "%2E", "%2E%2E"] {
        let encoded = ras_transport_core::encode_path_segment(value).expect("accepted value");
        let url = format!("https://api.example/items/{encoded}/delete");
        let request = reqwest::Client::new().delete(&url).build().unwrap();
        assert_eq!(
            request.url().path(),
            format!("/items/{encoded}/delete"),
            "{value:?} changed the endpoint path",
        );
    }
}

/// Logging a request with `{:?}` must not leak its credentials.
#[test]
fn request_debug_redacts_credentials() {
    let request = TransportRequest::new(Method::GET, "https://api.example")
        .bearer("bearer-secret")
        .unwrap()
        .header("proxy-authorization", "Basic proxy-secret")
        .header("cookie", "session=cookie-secret")
        .header("x-trace", "visible-value");
    let debug = format!("{request:?}");
    for secret in ["bearer-secret", "proxy-secret", "cookie-secret"] {
        assert!(!debug.contains(secret), "{secret} leaked: {debug}");
    }
    assert!(debug.contains("visible-value"), "{debug}");

    // An Authorization header set directly, not through `bearer`, is hidden too.
    let request =
        TransportRequest::new(Method::GET, "https://api.example").header("Authorization", "raw");
    assert!(!format!("{request:?}").contains("raw"));
}

fn chunked_response(status: u16, chunks: Vec<&'static [u8]>) -> TransportResponse {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<_, TransportError>(Bytes::from_static(chunk))),
    );
    TransportResponse::new(
        http::StatusCode::from_u16(status).unwrap(),
        http::HeaderMap::new(),
        byte_stream_from(stream),
    )
}

/// A server must not be able to make a client buffer without bound.
#[tokio::test]
async fn collecting_a_response_stops_at_the_body_limit() {
    assert_eq!(TransportResponse::DEFAULT_BODY_LIMIT, 16 * 1024 * 1024);

    let at_limit = chunked_response(200, vec![b"abc", b"def"]).with_body_limit(6);
    assert_eq!(at_limit.bytes().await.unwrap(), "abcdef");

    let over = chunked_response(200, vec![b"abc", b"defg"]).with_body_limit(6);
    assert!(matches!(
        over.bytes().await,
        Err(TransportError::BodyTooLarge { limit: 6 })
    ));

    let over = chunked_response(200, vec![b"\"abcdefg\""]).with_body_limit(6);
    assert!(matches!(
        over.json::<String>().await,
        Err(TransportError::BodyTooLarge { limit: 6 })
    ));

    let over = chunked_response(200, vec![b"abcdefg"]).with_body_limit(6);
    assert!(matches!(
        over.text().await,
        Err(TransportError::BodyTooLarge { limit: 6 })
    ));
}

/// The error body kept for a non-2xx answer is cut at the limit instead of
/// being read to the end.
#[tokio::test]
async fn error_for_status_truncates_a_long_error_body() {
    let response = chunked_response(500, vec![b"abc", b"def", b"ghi"]).with_body_limit(4);
    match response.error_for_status().await {
        Err(TransportError::Status { body, .. }) => assert_eq!(body, "abcd<truncated>"),
        other => panic!("expected a status error, got {other:?}"),
    }
}
