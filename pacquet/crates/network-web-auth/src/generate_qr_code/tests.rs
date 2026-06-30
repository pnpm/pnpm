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
