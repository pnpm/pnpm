use std::fmt::Write as _;

/// Decode percent-escaped UTF-8 without form-query semantics: `+` and `&`
/// remain literal. Invalid escapes pass through; invalid UTF-8 is replaced.
#[must_use]
pub fn percent_decode_str(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[idx + 1..idx + 3]).ok();
            if let Some(byte) = hex.and_then(|hex_digits| u8::from_str_radix(hex_digits, 16).ok()) {
                out.push(byte);
                idx += 3;
                continue;
            }
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a package name for a packument URL, matching pnpm's
/// `toUri`: a scoped name keeps its leading `@` and encodes the rest (so the
/// `/` becomes `%2F`), an unscoped name is encoded whole.
#[must_use]
pub fn encode_package_name(name: &str) -> String {
    match name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest)),
        None => encode_uri_component(name),
    }
}

/// Port of JavaScript `encodeURIComponent`: every UTF-8 byte outside the
/// unreserved set is percent-encoded.
#[must_use]
pub fn encode_uri_component(input: &str) -> String {
    const UNRESERVED: &[u8] = b"-_.!~*'()";
    let mut output = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || UNRESERVED.contains(&byte) {
            output.push(byte as char);
        } else {
            write!(output, "%{byte:02X}").expect("writing to a String never fails");
        }
    }
    output
}
