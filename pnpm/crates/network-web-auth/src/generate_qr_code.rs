use qrcode::{EcLevel, QrCode, Version, bits::Bits, types::Color};

/// Render `text` as a compact half-block Unicode QR code for a terminal,
/// matching the output of pnpm's `qrcode-terminal` small mode.
///
/// The code is encoded at error-correction level `L` in a single byte segment
/// (so its version — and therefore its on-screen size — matches pnpm's) and
/// framed by a one-module light border rather than the four-module quiet zone
/// the crate's own renderer draws, keeping the margin thin. Dark modules are
/// blank and light modules are block glyphs, so on the usual light-on-dark
/// terminal the code shows as dark modules inside a light frame — the
/// orientation a QR scanner expects.
///
/// Returns an error only when `text` exceeds the maximum QR data capacity.
/// `text` comes from an untrusted registry response, so an oversized payload
/// surfaces as a recoverable error rather than a panic.
pub fn generate_qr_code(text: &str) -> Result<String, GenerateQrCodeError> {
    byte_mode_code(text.as_bytes(), EcLevel::L).map(|code| render_small(&code))
}

/// Encode `data` as a single byte-mode segment at the smallest version that
/// fits, matching `qrcode-terminal`'s `addData`. The crate's `QrCode::new`
/// optimizes into mixed segments instead, which can pick a different version
/// (and so a different size) than pnpm renders.
fn byte_mode_code(data: &[u8], ec_level: EcLevel) -> Result<QrCode, GenerateQrCodeError> {
    for version_number in 1_i16..=40 {
        let mut bits = Bits::new(Version::Normal(version_number));
        if bits.push_byte_data(data).is_err() || bits.push_terminator(ec_level).is_err() {
            continue;
        }
        if let Ok(code) = QrCode::with_bits(bits, ec_level) {
            return Ok(code);
        }
    }
    Err(GenerateQrCodeError { reason: "text exceeds the maximum QR code data capacity".to_owned() })
}

/// Render `code` in `qrcode-terminal`'s small (half-block) style: a one-row
/// top border, each module row framed by a light column, and each character
/// stacking two vertical modules. A real QR always has an odd module count, so
/// the bottom edge is the last row's light lower half — no separate bottom
/// border row.
///
/// Every line (including the last) is terminated by a newline, as
/// `qrcode-terminal` does. That trailing newline is what separates the code
/// from a following prompt with a blank line.
fn render_small(code: &QrCode) -> String {
    let width = code.width();
    let colors = code.to_colors();
    // Rows past the bottom edge pair with a light module, so the final
    // character row's lower half is the (thin) bottom quiet zone.
    let is_dark = |row: usize, col: usize| row < width && colors[row * width + col] == Color::Dark;

    let mut out = String::with_capacity((width / 2 + 2) * (width + 3) * 3);
    out.push_str(&"\u{2584}".repeat(width + 2));
    out.push('\n');
    let mut row = 0;
    while row < width {
        out.push('\u{2588}');
        for col in 0..width {
            out.push(match (is_dark(row, col), is_dark(row + 1, col)) {
                (false, false) => '\u{2588}', // both light  -> full block
                (false, true) => '\u{2580}',  // light / dark -> upper half
                (true, false) => '\u{2584}',  // dark / light -> lower half
                (true, true) => ' ',          // both dark   -> blank
            });
        }
        out.push('\u{2588}');
        out.push('\n');
        row += 2;
    }
    out
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
