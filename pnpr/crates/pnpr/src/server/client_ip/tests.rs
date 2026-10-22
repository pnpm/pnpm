use super::{parse_hop_ip, resolve_client_ip};
use axum::http::{HeaderMap, HeaderValue};
use pnpr_config::{IpNetwork, TrustedProxyHeader};
use std::net::IpAddr;

fn ip(raw: &str) -> IpAddr {
    raw.parse().unwrap()
}

fn networks(entries: &[&str]) -> Vec<IpNetwork> {
    entries
        .iter()
        .map(|entry| IpNetwork::parse(entry).unwrap())
        .collect()
}

fn resolve_xff(peer: Option<IpAddr>, headers: &HeaderMap, trusted: &[IpNetwork]) -> Option<IpAddr> {
    resolve_client_ip(peer, headers, trusted, TrustedProxyHeader::XForwardedFor)
}

fn resolve_forwarded(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted: &[IpNetwork],
) -> Option<IpAddr> {
    resolve_client_ip(peer, headers, trusted, TrustedProxyHeader::Forwarded)
}

fn resolve_real_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted: &[IpNetwork],
) -> Option<IpAddr> {
    resolve_client_ip(peer, headers, trusted, TrustedProxyHeader::XRealIp)
}

#[test]
fn peer_none_returns_none() {
    let headers = HeaderMap::new();
    let trusted = networks(&["127.0.0.1"]);
    assert_eq!(resolve_xff(None, &headers, &trusted), None);
}

#[test]
fn untrusted_peer_ignores_forwarding_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
    headers.insert("forwarded", HeaderValue::from_static("for=10.0.0.1"));
    headers.insert("x-real-ip", HeaderValue::from_static("10.0.0.1"));

    let peer = ip("192.168.1.50");
    let trusted = networks(&["127.0.0.1", "10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), Some(peer));
}

#[test]
fn empty_trusted_proxies_ignores_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));

    let peer = ip("127.0.0.1");
    assert_eq!(resolve_xff(Some(peer), &headers, &[]), Some(peer));
}

#[test]
fn trusted_peer_resolves_x_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.1"));

    let peer = ip("127.0.0.1");
    let trusted = networks(&["127.0.0.1"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), Some(ip("203.0.113.1")));
}

#[test]
fn multi_hop_x_forwarded_for_right_to_left_traversal() {
    let mut headers = HeaderMap::new();
    // Spoofed: 1.1.1.1, Real client: 203.0.113.1, Proxy 1: 10.0.0.1, Proxy 2: 10.0.0.2
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("1.1.1.1, 203.0.113.1, 10.0.0.1, 10.0.0.2"),
    );

    let peer = ip("10.0.0.3"); // Immediate peer
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), Some(ip("203.0.113.1")));
}

#[test]
fn all_hops_trusted_picks_leftmost_hop() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1, 10.0.0.2"));

    let peer = ip("10.0.0.3");
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), Some(ip("10.0.0.1")));
}

#[test]
fn malformed_hop_fails_closed() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.1, bad_ip, 10.0.0.1"));

    let peer = ip("10.0.0.2");
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), None);
}

#[test]
fn injected_forwarded_is_ignored_when_x_forwarded_for_is_configured() {
    let mut headers = HeaderMap::new();
    headers.insert("forwarded", HeaderValue::from_static("for=10.1.2.3"));
    headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.1"));

    let peer = ip("127.0.0.1");
    let trusted = networks(&["127.0.0.1"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), Some(ip("203.0.113.1")));
}

#[test]
fn non_ascii_x_forwarded_for_fails_closed_without_falling_back_to_x_real_ip() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_bytes(b"\x80").unwrap());
    headers.insert("x-real-ip", HeaderValue::from_static("10.1.2.3"));

    let peer = ip("127.0.0.1");
    let trusted = networks(&["127.0.0.1"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), None);
}

#[test]
fn trusted_peer_resolves_forwarded_rfc7239() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=203.0.113.1;proto=https, for=10.0.0.1"),
    );
    headers.insert("x-forwarded-for", HeaderValue::from_static("1.1.1.1"));

    let peer = ip("10.0.0.2");
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_forwarded(Some(peer), &headers, &trusted), Some(ip("203.0.113.1")));
}

#[test]
fn quoted_forwarded_values_cannot_inject_synthetic_hops() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static(r#"foo="client, for=10.1.2.3;bar=baz", for=127.0.0.1"#),
    );

    let peer = ip("127.0.0.1");
    let trusted = networks(&["127.0.0.1"]);
    assert_eq!(resolve_forwarded(Some(peer), &headers, &trusted), None);
}

#[test]
fn trusted_peer_resolves_x_real_ip() {
    let mut headers = HeaderMap::new();
    headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.5"));
    headers.insert("x-forwarded-for", HeaderValue::from_static("1.1.1.1"));

    let peer = ip("127.0.0.1");
    let trusted = networks(&["127.0.0.1"]);
    assert_eq!(resolve_real_ip(Some(peer), &headers, &trusted), Some(ip("203.0.113.5")));
}

#[test]
fn parse_hop_ip_handles_various_formats() {
    assert_eq!(parse_hop_ip("192.0.2.1"), Some(ip("192.0.2.1")));
    assert_eq!(parse_hop_ip("192.0.2.1:8080"), Some(ip("192.0.2.1")));
    assert_eq!(parse_hop_ip("2001:db8::1"), Some(ip("2001:db8::1")));
    assert_eq!(parse_hop_ip("[2001:db8::1]"), Some(ip("2001:db8::1")));
    assert_eq!(parse_hop_ip("[2001:db8::1]:443"), Some(ip("2001:db8::1")));
    assert_eq!(parse_hop_ip(r#""192.0.2.1""#), Some(ip("192.0.2.1")));
    assert_eq!(parse_hop_ip("::ffff:192.0.2.1"), Some(ip("192.0.2.1")));
    assert_eq!(parse_hop_ip("192.0.2.1:notaport"), None);
    assert_eq!(parse_hop_ip("192.0.2.1:99999"), None);
    assert_eq!(parse_hop_ip("[2001:db8::1]junk"), None);
    assert_eq!(parse_hop_ip("[2001:db8::1]:notaport"), None);
    assert_eq!(parse_hop_ip(r#""192.0.2.1"#), None);
    assert_eq!(parse_hop_ip(r#"192.0.2.1""#), None);
    assert_eq!(parse_hop_ip(r#"""192.0.2.1"""#), None);
    assert_eq!(parse_hop_ip("unknown"), None);
    assert_eq!(parse_hop_ip(""), None);
}

#[test]
fn invalid_port_or_suffix_fails_closed() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("10.1.2.3:notaport, 10.0.0.1"));

    let peer = ip("10.0.0.2");
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), None);
}

#[test]
fn trailing_junk_after_bracket_fails_closed() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("[2001:db8::1]junk, 10.0.0.1"));

    let peer = ip("10.0.0.2");
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), None);
}

#[test]
fn unmatched_quotes_fail_closed() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static(r#""203.0.113.1, 10.0.0.1"#));

    let peer = ip("10.0.0.2");
    let trusted = networks(&["10.0.0.0/8"]);
    assert_eq!(resolve_xff(Some(peer), &headers, &trusted), None);
}
