use qrcode::{QrCode, render::unicode};

/// Render `text` as a compact half-block Unicode QR code for a terminal.
///
/// Light modules (including the quiet-zone border) are drawn as block
/// glyphs and dark modules as blank cells, so on the usual light-on-dark
/// terminal the code shows as dark modules inside a light frame — the
/// orientation a QR scanner expects. This is why the renderer's dark and
/// light colors are swapped from the crate default, which draws dark
/// modules as glyphs and would render inverted and borderless on a dark
/// terminal.
///
/// Returns an error only when `text` exceeds the maximum QR data capacity.
/// `text` comes from an untrusted registry response, so an oversized
/// payload surfaces as a recoverable error rather than a panic.
pub fn generate_qr_code(text: &str) -> Result<String, GenerateQrCodeError> {
    QrCode::new(text)
        .map(|code| {
            code.render::<unicode::Dense1x2>()
                .dark_color(unicode::Dense1x2::Light)
                .light_color(unicode::Dense1x2::Dark)
                .build()
        })
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
