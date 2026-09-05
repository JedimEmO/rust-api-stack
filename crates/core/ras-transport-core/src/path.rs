/// Percent-encode a value for safe interpolation into a single URL **path
/// segment**.
///
/// Generated clients substitute path parameters into a URL template by string
/// replacement (`/items/{id}` -> `/items/<value>`). Without encoding, a value
/// containing `/`, `?`, `#`, `%`, or control bytes could break out of its
/// segment and alter the request's path, query, or fragment (e.g. an `id` of
/// `../admin` or `x?role=admin`). Only RFC 3986 `unreserved` characters
/// (`ALPHA`/`DIGIT`/`-`/`.`/`_`/`~`) pass through unescaped; every other byte is
/// `%XX`-encoded. The result is what servers (e.g. axum's `Path` extractor)
/// percent-decode back to the original value.
pub fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &b in value.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
    out
}

/// Upper-case hex digit for a nibble (0..=15).
fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}
