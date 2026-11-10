//! Random (version 4) UUID strings, in the hyphenated form clients quote
//! back: five groups of hex digits, `8-4-4-4-12`.

use std::fmt::Write as _;

/// Two hex digits per byte, so these groups render as the familiar
/// 8-4-4-4-12.
const GROUP_BYTES: &[usize] = &[4, 2, 2, 2, 6];

/// Generate a fresh random (version 4) UUID from the OS CSPRNG.
pub(crate) fn generate() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    set_version_4(&mut bytes);
    set_rfc_9562_variant(&mut bytes);
    hyphenate(&bytes)
}

fn set_version_4(bytes: &mut [u8; 16]) {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
}

fn set_rfc_9562_variant(bytes: &mut [u8; 16]) {
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
}

/// Render `bytes` as hyphen-separated hex groups, sized by [`GROUP_BYTES`].
fn hyphenate(bytes: &[u8; 16]) -> String {
    let mut text = String::with_capacity(36);
    let mut rest = bytes.as_slice();
    for group_bytes in GROUP_BYTES {
        if !text.is_empty() {
            text.push('-');
        }
        let (group, remainder) = rest.split_at(*group_bytes);
        for byte in group {
            write!(text, "{byte:02x}").expect("writing to a String cannot fail");
        }
        rest = remainder;
    }
    text
}

#[cfg(test)]
mod tests;
