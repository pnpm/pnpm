use std::{fmt, sync::Arc};

/// A validated request identifier, either echoed from an incoming `x-request-id` header
/// or generated fresh as an RFC 4122 / RFC 9562 UUID v4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub Arc<str>);

impl RequestId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Validate whether an incoming `x-request-id` header value is safe to reuse.
///
/// Refuses empty values, strings longer than 128 characters, and any values
/// containing ASCII whitespace or control characters.
#[must_use]
pub(crate) fn is_valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| (33..=126).contains(&byte))
}

/// Generate a fresh RFC 4122 UUID v4 request identifier.
#[must_use]
pub(crate) fn generate_request_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG is available");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}
