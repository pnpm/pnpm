use super::generate_qr_code;

#[test]
fn returns_a_non_empty_string() {
    let qr = generate_qr_code("https://example.com").expect("encode a short URL");
    assert!(!qr.is_empty());
}

#[test]
fn produces_different_output_for_different_inputs() {
    let qr1 = generate_qr_code("https://example.com/a").expect("encode URL a");
    let qr2 = generate_qr_code("https://example.com/b").expect("encode URL b");
    assert_ne!(qr1, qr2);
}

/// The output ends with a newline (as `qrcode-terminal` does), so a following
/// prompt is separated from the code by a blank line.
#[test]
fn ends_with_a_trailing_newline() {
    let qr = generate_qr_code("https://example.com").expect("encode a short URL");
    assert!(qr.ends_with('\n'), "the QR code should end with a newline");
}

/// The data capacity matches `qrcode-terminal`'s: a single byte-mode
/// segment at error-correction level L tops out at 2953 bytes (version 40),
/// so pacquet renders exactly the payloads pnpm renders and fails — with a
/// recoverable error — exactly where pnpm's `generateQrCode` throws.
#[test]
fn accepts_data_up_to_qrcode_terminals_capacity_and_rejects_beyond() {
    generate_qr_code(&"a".repeat(2953)).expect("encode 2953 bytes (version 40 at EC level L)");
    generate_qr_code(&"a".repeat(2954)).expect_err("2954 bytes exceed every QR version");
}

/// The margin is a thin one-module border, matching pnpm's `qrcode-terminal`:
/// a single lower-half-block top row and a single light column framing each
/// module row — not the crate renderer's multi-row four-module quiet zone.
#[test]
fn renders_a_thin_one_module_border() {
    let qr = generate_qr_code("https://example.com").expect("encode a short URL");
    let lines: Vec<&str> = qr.lines().collect();
    assert!(
        lines[0].chars().all(|glyph| glyph == '\u{2584}'),
        "the top border should be one ▄ row, got {:?}",
        lines[0],
    );
    for line in &lines[1..] {
        assert!(
            line.starts_with('\u{2588}') && line.ends_with('\u{2588}'),
            "each module row should be framed by a single █ column, got {line:?}",
        );
    }
}
