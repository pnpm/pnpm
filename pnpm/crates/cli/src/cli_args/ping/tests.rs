use super::redact_and_sanitize;

#[test]
fn redact_and_sanitize_strips_credentials_and_control_chars() {
    // A `reqwest` error that echoes a registry URL with inline basic-auth
    // and an attacker-injected escape sequence + newline.
    let dirty =
        "error sending request for url (https://user:pass@host/-/ping): \u{1b}[31m\nPONG spoofed";
    let clean = redact_and_sanitize(dirty);
    assert!(!clean.contains("user:pass"), "credentials must be redacted: {clean:?}");
    assert!(!clean.contains('\u{1b}'), "escape sequences must be stripped: {clean:?}");
    assert!(!clean.contains('\n'), "newlines must be stripped: {clean:?}");
    assert!(clean.contains("https://host/-/ping"), "non-sensitive text is kept: {clean:?}");
}
