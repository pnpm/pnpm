use pacquet_reporter::Reporter;

use crate::{generate_qr_code::generate_qr_code, global_log::global_warn};

/// Format the "Authenticate your account at" message for `auth_url`,
/// appending a QR code rendering of it when one can be generated.
///
/// The URL itself is the authentication mechanism and the QR code only a
/// convenience, so a QR generation failure (e.g. a URL exceeding the
/// maximum QR data capacity) downgrades to a global warning and a URL-only
/// message instead of aborting the authentication flow.
pub fn format_auth_url_message<Reporter: self::Reporter>(auth_url: &str) -> String {
    match generate_qr_code(auth_url) {
        Ok(qr_code) => format!("Authenticate your account at:\n{auth_url}\n\n{qr_code}"),
        Err(error) => {
            global_warn::<Reporter>(&format!("Could not generate a QR code: {error}"));
            format!("Authenticate your account at:\n{auth_url}")
        }
    }
}
