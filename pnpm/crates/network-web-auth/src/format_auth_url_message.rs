use pacquet_reporter::Reporter;

use crate::{generate_qr_code::generate_qr_code, global_log::global_warn};

/// The "Authenticate your account at" message, as a lazily-rendered
/// [`Display`](std::fmt::Display) value rather than an owned `String`.
///
/// Deferring the formatting lets the reporter write the message straight
/// into its one `GlobalLog` allocation, instead of this function building a
/// concatenated `String` that the reporter would then copy again — the QR
/// block is several kilobytes, so the extra copy is worth avoiding.
#[derive(derive_more::Display)]
pub enum AuthUrlMessage<'a> {
    #[display("Authenticate your account at:\n{auth_url}\n\n{qr_code}")]
    WithQrCode { auth_url: &'a str, qr_code: String },
    #[display("Authenticate your account at:\n{auth_url}")]
    UrlOnly { auth_url: &'a str },
}

/// Build the [`AuthUrlMessage`] for `auth_url`, rendering it with a QR code
/// when one can be generated.
///
/// The URL itself is the authentication mechanism and the QR code only a
/// convenience, so a QR generation failure (e.g. a URL exceeding the
/// maximum QR data capacity) downgrades to a global warning and a URL-only
/// message instead of aborting the authentication flow.
#[must_use]
pub fn format_auth_url_message<Reporter: self::Reporter>(auth_url: &str) -> AuthUrlMessage<'_> {
    match generate_qr_code(auth_url) {
        Ok(qr_code) => AuthUrlMessage::WithQrCode { auth_url, qr_code },
        Err(error) => {
            global_warn::<Reporter>(format!("Could not generate a QR code: {error}"));
            AuthUrlMessage::UrlOnly { auth_url }
        }
    }
}
