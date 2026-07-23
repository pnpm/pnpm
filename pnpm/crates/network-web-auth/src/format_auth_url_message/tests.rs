use pacquet_reporter::{LogEvent, Reporter};
use pretty_assertions::assert_eq;

use super::{AuthUrlMessage, format_auth_url_message};
use crate::generate_qr_code::generate_qr_code;

/// Fails the test on any log message: the QR-code happy path renders the
/// message without warning, so its counterpart on the TypeScript side passes
/// a `globalWarn` that throws.
struct UnexpectedReporter;

impl Reporter for UnexpectedReporter {
    fn emit(event: &LogEvent) {
        panic!("unexpected log: {event:?}")
    }
}

/// Mirrors the TypeScript `formatAuthUrlMessage` "appends a QR code" test, so a
/// regression that drops the code from the message is caught in both stacks.
#[test]
fn renders_the_auth_url_with_its_qr_code() {
    let auth_url = "https://example.com/auth";
    let qr_code = generate_qr_code(auth_url).expect("a short URL encodes");
    let message = format_auth_url_message::<UnexpectedReporter>(auth_url);
    assert!(matches!(message, AuthUrlMessage::WithQrCode { .. }), "expected a QR-code message");
    assert_eq!(
        message.to_string(),
        format!("Authenticate your account at:\n{auth_url}\n\n{qr_code}"),
    );
}
