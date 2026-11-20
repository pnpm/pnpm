use super::TrustedProxyHeader;

fn parse(raw: &str) -> TrustedProxyHeader {
    raw.parse().unwrap()
}

#[test]
fn parses_supported_headers() {
    assert_eq!(parse("forwarded"), TrustedProxyHeader::Forwarded);
    assert_eq!(parse("Forwarded"), TrustedProxyHeader::Forwarded);
    assert_eq!(parse("x-forwarded-for"), TrustedProxyHeader::XForwardedFor);
    assert_eq!(parse("X-Forwarded-For"), TrustedProxyHeader::XForwardedFor);
    assert_eq!(parse("x_forwarded_for"), TrustedProxyHeader::XForwardedFor);
    assert_eq!(parse("x-real-ip"), TrustedProxyHeader::XRealIp);
    assert_eq!(parse("X-Real-IP"), TrustedProxyHeader::XRealIp);
    assert_eq!(parse("x_real_ip"), TrustedProxyHeader::XRealIp);
}

#[test]
fn rejects_unknown_headers() {
    assert!("x-custom-ip".parse::<TrustedProxyHeader>().is_err());
    assert!("".parse::<TrustedProxyHeader>().is_err());
}
