use qrcode::{QrCode, render::unicode};

/// Render `text` as a compact Unicode QR code for printing in a terminal.
///
/// Ports pnpm's `generateQrCode`, which used `qrcode-terminal` with
/// `small: true`. The exact glyphs differ (this uses `qrcode`'s
/// half-block `Dense1x2` renderer), but the encoded payload and
/// scannability match.
///
/// Returns an error only when `text` is too long to fit in any QR version
/// — pnpm treats this as a should-never-happen, but `text` comes from an
/// untrusted registry response, so this surfaces it as a recoverable error
/// rather than a panic.
pub fn generate_qr_code(text: &str) -> Result<String, GenerateQrCodeError> {
    QrCode::new(text)
        .map(|code| code.render::<unicode::Dense1x2>().build())
        .map_err(|source| GenerateQrCodeError { reason: source.to_string() })
}

/// `text` could not be encoded as a QR code (e.g. it exceeds the maximum
/// QR data capacity).
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("failed to generate a QR code: {reason}")]
pub struct GenerateQrCodeError {
    reason: String,
}

#[cfg(test)]
mod tests;
