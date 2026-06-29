use std::fmt::Write as _;

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
