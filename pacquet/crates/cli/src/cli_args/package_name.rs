use std::fmt::Write as _;

pub(crate) fn encode_package_name(name: &str) -> String {
    match name.strip_prefix('@') {
        Some(rest) => format!("@{}", encode_uri_component(rest)),
        None => encode_uri_component(name),
    }
}

pub(crate) fn encode_uri_component(input: &str) -> String {
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
