use super::validate_destination;

#[test]
fn refuses_nonpublic_literals() {
    for url in ["https://127.0.0.1/jwks", "https://[::1]/token", "https://[::ffff:10.0.0.1]/jwks"] {
        assert!(validate_destination(url).is_err(), "accepted {url}");
    }
    assert!(validate_destination("https://8.8.8.8/jwks").is_ok());
    assert!(validate_destination("https://issuer.example/jwks").is_ok());
}
