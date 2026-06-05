//! In-process unit coverage for the request/response value types, the typed
//! error, and the query-serialization helpers. No transport, no sockets.

use bytes::Bytes;
use futures_util::stream;
use http::{Method, StatusCode};
use ras_transport_core::request::RequestBody;
use ras_transport_core::{
    ByteStream, TransportError, TransportRequest, TransportResponse, byte_stream_from,
    deserialize_json, serialize_query_pairs, serialize_query_value,
};
use serde::Serialize;

fn single_chunk(bytes: &'static [u8]) -> ByteStream {
    byte_stream_from(stream::once(async move {
        Ok::<Bytes, TransportError>(Bytes::from_static(bytes))
    }))
}

fn response(status: u16, body: &'static [u8]) -> TransportResponse {
    TransportResponse::new(
        StatusCode::from_u16(status).unwrap(),
        http::HeaderMap::new(),
        single_chunk(body),
    )
}

// --- TransportRequest builders ---

#[test]
fn request_builders_set_method_url_headers_body_timeout() {
    #[derive(Serialize)]
    struct Payload {
        a: u32,
    }

    let req = TransportRequest::new(Method::POST, "http://h/x")
        .header("x-custom", "v")
        .bearer("tok")
        .json(&Payload { a: 1 })
        .expect("json body")
        .timeout(std::time::Duration::from_millis(50));

    assert_eq!(req.method, Method::POST);
    assert_eq!(req.url, "http://h/x");
    assert_eq!(req.headers.get("x-custom").unwrap(), "v");
    assert_eq!(req.headers.get("authorization").unwrap(), "Bearer tok");
    assert_eq!(req.headers.get("content-type").unwrap(), "application/json");
    assert_eq!(req.timeout, Some(std::time::Duration::from_millis(50)));
    assert!(matches!(req.body, RequestBody::Bytes(_)));
}

#[test]
fn request_header_drops_invalid_name_or_value() {
    // Invalid header name (contains a space) and invalid value (newline) are
    // silently dropped rather than panicking.
    let req = TransportRequest::new(Method::GET, "/x")
        .header("bad name", "ok")
        .header("ok-name", "bad\nvalue");
    assert!(req.headers.is_empty());
}

#[test]
fn request_body_direct_and_empty() {
    let req = TransportRequest::new(Method::PUT, "/x").body(RequestBody::Bytes(Bytes::from_static(b"hi")));
    assert!(matches!(req.body, RequestBody::Bytes(_)));

    assert!(matches!(RequestBody::empty(), RequestBody::Empty));
}

#[test]
fn request_body_debug_variants() {
    assert_eq!(format!("{:?}", RequestBody::Empty), "RequestBody::Empty");
    assert_eq!(
        format!("{:?}", RequestBody::Bytes(Bytes::from_static(b"abc"))),
        "RequestBody::Bytes(3)"
    );
    let s = RequestBody::Stream(single_chunk(b"x"));
    assert_eq!(format!("{s:?}"), "RequestBody::Stream(..)");
}

// --- TransportResponse ---

#[tokio::test]
async fn response_bytes_text_json_and_accessors() {
    let resp = response(200, b"{\"v\":7}");
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.is_success());
    assert!(resp.headers().is_empty());
    assert!(format!("{resp:?}").contains("ByteStream(..)"));

    #[derive(serde::Deserialize)]
    struct V {
        v: u32,
    }
    let parsed: V = response(200, b"{\"v\":7}").json().await.unwrap();
    assert_eq!(parsed.v, 7);

    assert_eq!(response(200, b"hello").text().await.unwrap(), "hello");
    assert_eq!(&response(200, b"raw").bytes().await.unwrap()[..], b"raw");
}

#[tokio::test]
async fn response_into_body_stream_yields_chunks() {
    use futures_util::StreamExt;
    let chunks: Vec<&'static [u8]> = vec![b"foo", b"bar", b"baz"];
    let body = byte_stream_from(stream::iter(
        chunks
            .into_iter()
            .map(|c| Ok::<Bytes, TransportError>(Bytes::from_static(c))),
    ));
    let resp = TransportResponse::new(StatusCode::OK, http::HeaderMap::new(), body);

    let mut stream = resp.into_body_stream();
    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        collected.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(collected, b"foobarbaz");
}

#[tokio::test]
async fn error_for_status_passes_success_and_maps_failure() {
    // success: returns self unchanged
    let ok = response(204, b"").error_for_status().await.expect("2xx ok");
    assert_eq!(ok.status(), StatusCode::NO_CONTENT);

    // failure: drains body into TransportError::Status
    let err = response(503, b"down")
        .error_for_status()
        .await
        .expect_err("5xx errors");
    match err {
        TransportError::Status { status, body } => {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body, "down");
        }
        other => panic!("expected Status, got {other:?}"),
    }
}

// --- TransportError ---

#[test]
fn error_display_and_constructor_cover_all_variants() {
    let status = TransportError::http_status(StatusCode::NOT_FOUND, "missing");
    assert_eq!(status.to_string(), "http status 404 Not Found: missing");

    assert_eq!(
        TransportError::Connection("refused".into()).to_string(),
        "connection error: refused"
    );
    assert_eq!(
        TransportError::Body("truncated".into()).to_string(),
        "body error: truncated"
    );
    assert_eq!(
        TransportError::JsonRpc {
            code: -32601,
            message: "Method not found".into()
        }
        .to_string(),
        "json-rpc error -32601: Method not found"
    );

    // Deserialize variant: invalid JSON bytes.
    let de_err: Result<u32, _> = deserialize_json(b"not json");
    assert!(matches!(de_err, Err(TransportError::Deserialize(_))));

    // Serialize variant: serde_json rejects a map with non-string keys.
    let mut bad = std::collections::BTreeMap::new();
    bad.insert((1u8, 2u8), 3u32);
    let ser_err = RequestBody::from_json(&bad).map(|_| ());
    assert!(matches!(ser_err, Err(TransportError::Serialize(_))));
}

// --- query helpers (lib.rs) ---

#[test]
fn query_value_covers_scalar_kinds_and_percent_decode() {
    // bool / int / float go through encode_scalar + percent_decode.
    assert_eq!(serialize_query_value("b", &true).unwrap(), vec![("b".into(), "true".into())]);
    assert_eq!(serialize_query_value("n", &-42i64).unwrap(), vec![("n".into(), "-42".into())]);
    assert_eq!(serialize_query_value("f", &1.5f64).unwrap(), vec![("f".into(), "1.5".into())]);

    // char '/' encodes to %2F then is percent-decoded back to '/'.
    assert_eq!(serialize_query_value("c", &'/').unwrap(), vec![("c".into(), "/".into())]);
    // a string value is taken verbatim (serialize_str path, no encode_scalar).
    assert_eq!(
        serialize_query_value("s", &"a b/c").unwrap(),
        vec![("s".into(), "a b/c".into())]
    );

    // newtype struct delegates to inner.
    #[derive(Serialize)]
    struct Wrap(u8);
    assert_eq!(serialize_query_value("w", &Wrap(9)).unwrap(), vec![("w".into(), "9".into())]);
}

#[test]
fn query_value_covers_every_scalar_width() {
    // Each integer/float width is a distinct generated Serializer method.
    assert_eq!(serialize_query_value("v", &7i8).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7i16).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7i32).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7i64).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7u8).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7u16).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7u32).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &7u64).unwrap()[0].1, "7");
    assert_eq!(serialize_query_value("v", &2.5f32).unwrap()[0].1, "2.5");
    assert_eq!(serialize_query_value("v", &2.5f64).unwrap()[0].1, "2.5");
}

#[test]
fn query_value_seq_option_and_unit_variant() {
    // Vec -> repeated values for one key.
    assert_eq!(
        serialize_query_value("t", &vec!["x", "y"]).unwrap(),
        vec![("t".into(), "x".into()), ("t".into(), "y".into())]
    );
    // Option::None -> no pairs; Some -> one.
    assert_eq!(serialize_query_value("o", &Option::<u32>::None).unwrap(), vec![]);
    assert_eq!(serialize_query_value("o", &Some(3u32)).unwrap(), vec![("o".into(), "3".into())]);

    #[derive(Serialize)]
    enum Kind {
        #[serde(rename = "the_one")]
        One,
    }
    assert_eq!(serialize_query_value("k", &Kind::One).unwrap(), vec![("k".into(), "the_one".into())]);
}

#[test]
fn query_value_tuple_newtype_variant_and_unit_shapes() {
    // Tuple -> repeated values (serialize_tuple + SerializeTuple).
    assert_eq!(
        serialize_query_value("t", &(1u32, 2u32)).unwrap(),
        vec![("t".into(), "1".into()), ("t".into(), "2".into())]
    );

    // Tuple struct -> repeated values (serialize_tuple_struct + SerializeTupleStruct).
    #[derive(Serialize)]
    struct Pair(u32, u32);
    assert_eq!(
        serialize_query_value("p", &Pair(3, 4)).unwrap(),
        vec![("p".into(), "3".into()), ("p".into(), "4".into())]
    );

    // Newtype variant -> delegates to inner value.
    #[derive(Serialize)]
    enum E {
        N(u32),
    }
    assert_eq!(serialize_query_value("e", &E::N(5)).unwrap(), vec![("e".into(), "5".into())]);

    // Unit and unit struct -> no pairs.
    #[derive(Serialize)]
    struct U;
    assert_eq!(serialize_query_value("u", &()).unwrap(), vec![]);
    assert_eq!(serialize_query_value("u", &U).unwrap(), vec![]);
}

#[test]
fn query_value_rejects_unsupported_shapes() {
    use std::collections::BTreeMap;

    #[derive(Serialize)]
    struct S {
        a: u32,
    }
    assert!(serialize_query_value("s", &S { a: 1 }).is_err()); // struct

    let mut m = BTreeMap::new();
    m.insert("k", 1);
    assert!(serialize_query_value("m", &m).is_err()); // map

    #[derive(Serialize)]
    enum E {
        Tup(u8, u8),
        Strukt { x: u8 },
    }
    assert!(serialize_query_value("e", &E::Tup(1, 2)).is_err()); // tuple variant
    assert!(serialize_query_value("e", &E::Strukt { x: 1 }).is_err()); // struct variant
}

#[test]
fn query_pairs_join_and_empty() {
    assert_eq!(serialize_query_pairs(&[]), "");
    let pairs = vec![("a".to_string(), "1".to_string()), ("b".to_string(), "two".to_string())];
    assert_eq!(serialize_query_pairs(&pairs), "a=1&b=two");
}
