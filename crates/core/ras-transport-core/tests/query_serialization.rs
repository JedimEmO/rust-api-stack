//! Query serialization must reproduce reqwest's `.query()` behavior:
//! Option-skipping, Vec repeated-keys, and enum serde-renames.

use ras_transport_core::{serialize_query_pairs, serialize_query_value};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Sort {
    #[serde(rename = "created_at")]
    CreatedAt,
    Name,
}

#[test]
fn option_none_skips_and_some_emits() {
    let none: Option<u32> = None;
    let pairs = serialize_query_value("limit", &none).unwrap();
    assert!(pairs.is_empty(), "None must produce no pairs: {pairs:?}");

    let some: Option<u32> = Some(10);
    let pairs = serialize_query_value("limit", &some).unwrap();
    assert_eq!(pairs, vec![("limit".to_string(), "10".to_string())]);
}

#[test]
fn vec_produces_repeated_keys() {
    let tags = vec!["a", "b", "c"];
    let pairs = serialize_query_value("tag", &tags).unwrap();
    assert_eq!(
        pairs,
        vec![
            ("tag".to_string(), "a".to_string()),
            ("tag".to_string(), "b".to_string()),
            ("tag".to_string(), "c".to_string()),
        ]
    );
    assert_eq!(serialize_query_pairs(&pairs), "tag=a&tag=b&tag=c");
}

#[test]
fn enum_uses_serde_rename() {
    let pairs = serialize_query_value("sort", &Sort::CreatedAt).unwrap();
    assert_eq!(pairs, vec![("sort".to_string(), "created_at".to_string())]);

    let pairs = serialize_query_value("sort", &Sort::Name).unwrap();
    assert_eq!(pairs, vec![("sort".to_string(), "name".to_string())]);
}

#[test]
fn pairs_are_percent_encoded() {
    let pairs = serialize_query_value("q", &"hello world & co").unwrap();
    assert_eq!(
        serialize_query_pairs(&pairs),
        "q=hello+world+%26+co"
    );
}

#[test]
fn encoding_matches_reqwests_urlencoded_unreserved_set() {
    // reqwest's `.query()` delegates to serde_urlencoded, whose unreserved set
    // is `[A-Za-z0-9*-._]` (space -> `+`). Regression coverage for the two
    // characters where the previous hand-rolled encoder diverged:
    //   `~` must be percent-encoded (`%7E`), and `*` must stay raw (`*`).
    let pairs = serialize_query_value("q", &"~").unwrap();
    assert_eq!(serialize_query_pairs(&pairs), "q=%7E");

    let pairs = serialize_query_value("q", &"*").unwrap();
    assert_eq!(serialize_query_pairs(&pairs), "q=*");

    // And the round-trip through `serialize_query_value` (which decodes the
    // scalar) followed by `serialize_query_pairs` (which re-encodes) is stable.
    let pairs = serialize_query_value("q", &"a~b*c").unwrap();
    assert_eq!(pairs, vec![("q".to_string(), "a~b*c".to_string())]);
    assert_eq!(serialize_query_pairs(&pairs), "q=a%7Eb*c");
}

#[test]
fn full_query_string_joins_multiple_params() {
    let mut all = Vec::new();
    all.extend(serialize_query_value("limit", &Some(5u32)).unwrap());
    let none: Option<u32> = None;
    all.extend(serialize_query_value("offset", &none).unwrap());
    all.extend(serialize_query_value("tag", &vec!["x", "y"]).unwrap());
    assert_eq!(serialize_query_pairs(&all), "limit=5&tag=x&tag=y");
}
