use crate::TransportError;
use serde::Serialize;

/// Convert a query value into decoded `(key, value)` pairs for form encoding.
///
/// Sequences produce repeated keys, enum variants honor `#[serde(rename)]`,
/// and `Option::None` produces no pairs. Encode with [`serialize_query_pairs`].
pub fn serialize_query_value<T: Serialize>(
    key: &str,
    value: &T,
) -> Result<Vec<(String, String)>, TransportError> {
    let mut collector = QueryValueCollector { values: Vec::new() };
    value
        .serialize(&mut collector)
        .map_err(|e| TransportError::Serialize(serde::ser::Error::custom(e.to_string())))?;
    Ok(collector
        .values
        .into_iter()
        .map(|v| (key.to_string(), v))
        .collect())
}

/// Serialize several `(key, value)` query parameters and join them into a
/// single query string (without a leading `?`). Empty result if no pairs.
///
/// Uses `application/x-www-form-urlencoded` encoding: `*` stays raw, `~`
/// becomes `%7E`, and space becomes `+`. Pair order, including repeated keys,
/// is preserved.
///
/// Returns [`TransportError::Serialize`] on encoding failure rather than
/// silently yielding an empty string — generated clients append the result
/// after a `?`/`&` separator, so a swallowed failure would send a different
/// (unfiltered) query than the caller asked for.
pub fn serialize_query_pairs(pairs: &[(String, String)]) -> Result<String, TransportError> {
    serde_urlencoded::to_string(pairs)
        .map_err(|e| TransportError::Serialize(serde::ser::Error::custom(e.to_string())))
}

/// Preserve form-serializer scalar formatting and enum renames while returning
/// a decoded value for [`serialize_query_pairs`].
fn encode_scalar<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    use serde::ser::Error as _;
    // serde_urlencoded serializes a sequence of (key, value) tuples.
    let encoded = serde_urlencoded::to_string([("v", value)])
        .map_err(|e| serde_json::Error::custom(e.to_string()))?;
    // `encoded` looks like "v=<encoded-value>"; strip the "v=" prefix and decode.
    let raw = encoded.strip_prefix("v=").unwrap_or(&encoded);
    Ok(percent_decode(raw))
}

/// A serde `Serializer` that collects scalar query values, expanding sequences
/// into multiple values and treating `Option::None`/unit as empty.
struct QueryValueCollector {
    values: Vec<String>,
}

type QueryResult = Result<(), serde_json::Error>;

macro_rules! collect_scalar {
    ($method:ident, $ty:ty) => {
        fn $method(self, v: $ty) -> QueryResult {
            self.values.push(encode_scalar(&v)?);
            Ok(())
        }
    };
}

impl serde::Serializer for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = serde::ser::Impossible<(), serde_json::Error>;
    type SerializeMap = serde::ser::Impossible<(), serde_json::Error>;
    type SerializeStruct = serde::ser::Impossible<(), serde_json::Error>;
    type SerializeStructVariant = serde::ser::Impossible<(), serde_json::Error>;

    collect_scalar!(serialize_bool, bool);
    collect_scalar!(serialize_i8, i8);
    collect_scalar!(serialize_i16, i16);
    collect_scalar!(serialize_i32, i32);
    collect_scalar!(serialize_i64, i64);
    collect_scalar!(serialize_u8, u8);
    collect_scalar!(serialize_u16, u16);
    collect_scalar!(serialize_u32, u32);
    collect_scalar!(serialize_u64, u64);
    collect_scalar!(serialize_f32, f32);
    collect_scalar!(serialize_f64, f64);
    collect_scalar!(serialize_char, char);

    fn serialize_str(self, v: &str) -> QueryResult {
        self.values.push(v.to_string());
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> QueryResult {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "bytes are not a valid query value",
        ))
    }

    fn serialize_none(self) -> QueryResult {
        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> QueryResult {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> QueryResult {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> QueryResult {
        // Honor `#[serde(rename = ...)]`: `variant` is already the renamed form.
        self.values.push(variant.to_string());
        Ok(())
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "tuple variants are not valid query values",
        ))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom("maps are not valid query values"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "structs are not valid query values",
        ))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        use serde::ser::Error as _;
        Err(serde_json::Error::custom(
            "struct variants are not valid query values",
        ))
    }
}

impl serde::ser::SerializeSeq for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_element<T>(&mut self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }
    fn end(self) -> QueryResult {
        Ok(())
    }
}

impl serde::ser::SerializeTuple for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_element<T>(&mut self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }
    fn end(self) -> QueryResult {
        Ok(())
    }
}

impl serde::ser::SerializeTupleStruct for &mut QueryValueCollector {
    type Ok = ();
    type Error = serde_json::Error;
    fn serialize_field<T>(&mut self, value: &T) -> QueryResult
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }
    fn end(self) -> QueryResult {
        Ok(())
    }
}

/// Decode `application/x-www-form-urlencoded` text (`+` -> space, `%XX`).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h << 4) | l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
