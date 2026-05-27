//! Tests for [`super`]'s proxy plumbing.
//!
//! Mirrors the describe blocks in pnpm v11's
//! [`network/fetch/test/dispatcher.test.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/network/fetch/test/dispatcher.test.ts)
//! that don't require a real proxy listener:
//!
//! * `HTTP proxy` — per-URL routing, basic-auth decoding, scheme bypass.
//! * `SOCKS proxy` — routing decision (live-network case skipped, same as
//!   upstream).
//! * `noProxy` — reverse-dot-segment match, bypass-all literal.
//! * `Invalid proxy URL` — `ERR_PNPM_INVALID_PROXY`.
//!
//! The one HTTP integration test stands up a [`mockito`] server playing
//! the role of an HTTP proxy and asserts the request arrives with an
//! absolute-form URI and a decoded `Proxy-Authorization` header.

use super::{
    ForInstallsError, NoProxyMatcher, NoProxySetting, PerRegistryTls, ProxyConfig, ProxyError,
    ThrottledClient, TlsConfig, parse_proxy_url,
};
use crate::proxy::{percent_decode_str, strip_userinfo};
use reqwest::Url;

fn list(entries: &[&str]) -> NoProxySetting {
    NoProxySetting::List(entries.iter().map(|s| (*s).to_string()).collect())
}

#[test]
fn no_proxy_matcher_reverse_dot_match() {
    let m = NoProxyMatcher::from(Some(&list(&["npmjs.org"])));
    // The matcher state is the same across every probe; logging it
    // once per test makes a failure diagnosable without rerunning.
    eprintln!("matcher={m:?}");
    for (host, expected) in [
        ("npmjs.org", true),
        ("registry.npmjs.org", true),
        ("foo.bar.npmjs.org", true),
        ("evilnpmjs.org", false),
        ("org", false),
    ] {
        let got = m.matches_host(host);
        assert_eq!(got, expected, "host={host}: expected match={expected}, got={got}");
    }
}

#[test]
fn no_proxy_matcher_empty_entries_never_match() {
    // Trailing/leading commas in `.npmrc` already get filtered in the
    // config layer's `parse_no_proxy`, but a malformed `List(vec![""])`
    // must still fail to match — defense in depth at the matcher.
    let m = NoProxyMatcher::from(Some(&list(&[""])));
    let got = m.matches_host("anything.example");
    assert!(!got, "matcher={m:?} host=anything.example expected miss, got match");
}

#[test]
fn no_proxy_matcher_multiple_entries() {
    let m = NoProxyMatcher::from(Some(&list(&["npmjs.org", "internal.example"])));
    eprintln!("matcher={m:?}");
    for (host, expected) in
        [("registry.npmjs.org", true), ("ci.internal.example", true), ("public.example", false)]
    {
        let got = m.matches_host(host);
        assert_eq!(got, expected, "host={host}: expected={expected}, got={got}");
    }
}

#[test]
fn no_proxy_bypass_short_circuits_every_host() {
    let m = NoProxyMatcher::from(Some(&NoProxySetting::Bypass));
    eprintln!("matcher={m:?}");
    for host in ["any.host", ""] {
        let got = m.matches_host(host);
        assert!(got, "host={host:?}: bypass must match every host, got miss");
    }
}

#[test]
fn no_proxy_none_matches_nothing() {
    let m = NoProxyMatcher::from(None);
    let got = m.matches_host("registry.npmjs.org");
    assert!(!got, "matcher={m:?}: None setting must never match");
}

#[test]
fn parse_proxy_url_auto_prefixes_missing_scheme() {
    // pnpm-parity: `proxy.example:8080` is treated as
    // `http://proxy.example:8080`.
    let url = parse_proxy_url("proxy.example:8080").expect("parses with retry");
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("proxy.example"));
    assert_eq!(url.port(), Some(8080));
}

#[test]
fn parse_proxy_url_keeps_existing_scheme() {
    let url = parse_proxy_url("https://proxy.example:8080").expect("parses");
    assert_eq!(url.scheme(), "https");
}

#[test]
fn parse_proxy_url_socks_schemes_pass_through() {
    // pnpm honors socks4, socks4a, socks5 (dispatcher.ts:124-132).
    // Routing happens elsewhere; here we only assert the URL parses.
    for scheme in ["socks4", "socks4a", "socks5"] {
        let url =
            parse_proxy_url(&format!("{scheme}://socksproxy.example:1080")).expect("socks parses");
        assert_eq!(url.scheme(), scheme);
    }
}

#[test]
fn parse_proxy_url_invalid_returns_invalid_proxy_error() {
    // `://` is malformed regardless of which scheme is prefixed.
    let err = parse_proxy_url("://broken").expect_err("malformed value must error");
    eprintln!("err={err:?}");
    match &err {
        ProxyError::InvalidProxy { url, .. } => assert_eq!(url, "://broken"),
    }
    // Diagnostic code matches upstream `ERR_PNPM_INVALID_PROXY`.
    let code = miette::Diagnostic::code(&err).expect("code() set");
    assert_eq!(code.to_string(), "ERR_PNPM_INVALID_PROXY");
}

#[test]
fn percent_decode_handles_common_escapes() {
    assert_eq!(percent_decode_str("p%40ss"), "p@ss", "%40 → @");
    assert_eq!(percent_decode_str("user%20name"), "user name");
    assert_eq!(percent_decode_str("plain"), "plain");
    assert_eq!(
        percent_decode_str("bad-%ZZ-escape"),
        "bad-%ZZ-escape",
        "invalid hex passes through",
    );
}

#[test]
fn strip_userinfo_decodes_user_and_password() {
    let url = Url::parse("http://us%40er:p%40ss@proxy.example:8080").expect("parse");
    let (clean, auth) = strip_userinfo(url);
    assert_eq!(clean.as_str(), "http://proxy.example:8080/");
    let (user, pass) = auth.expect("userinfo present");
    assert_eq!(user, "us@er", "user percent-decoded");
    assert_eq!(pass, "p@ss", "password percent-decoded");
}

#[test]
fn strip_userinfo_returns_none_when_absent() {
    let url = Url::parse("http://proxy.example:8080").expect("parse");
    let (clean, auth) = strip_userinfo(url.clone());
    assert_eq!(clean, url);
    let is_none = auth.is_none();
    assert!(is_none, "auth={auth:?}: expected None on URL without userinfo");
}

#[test]
fn for_installs_with_empty_proxy_config_builds() {
    // The legacy `new_for_installs` is now a wrapper around this — assert
    // the default `ProxyConfig` round-trips without error.
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
    )
    .expect("empty proxy is valid");
}

#[test]
fn for_installs_with_valid_proxy_url_builds() {
    let proxy = ProxyConfig {
        https_proxy: Some("http://proxy.example:8080".into()),
        http_proxy: Some("http://proxy.example:8080".into()),
        no_proxy: None,
    };
    ThrottledClient::for_installs(&proxy, &TlsConfig::default(), &PerRegistryTls::default())
        .expect("valid proxy URLs build");
}

#[test]
fn for_installs_with_invalid_proxy_url_errors() {
    let proxy =
        ProxyConfig { https_proxy: Some("://nonsense".into()), http_proxy: None, no_proxy: None };
    let err =
        ThrottledClient::for_installs(&proxy, &TlsConfig::default(), &PerRegistryTls::default())
            .expect_err("must error");
    eprintln!("err={err:?}");
    let is_invalid = matches!(err, ForInstallsError::Proxy(ProxyError::InvalidProxy { .. }));
    assert!(is_invalid, "err={err:?}: expected ForInstallsError::Proxy(InvalidProxy)");
}

#[test]
fn for_installs_with_socks_proxy_url_builds() {
    // Smoke test that the `socks` reqwest feature is wired correctly —
    // a socks URL must not be rejected at parse time, and the client
    // must build.
    let proxy = ProxyConfig {
        https_proxy: Some("socks5://socksproxy.example:1080".into()),
        http_proxy: None,
        no_proxy: None,
    };
    ThrottledClient::for_installs(&proxy, &TlsConfig::default(), &PerRegistryTls::default())
        .expect("socks proxy URL builds");
}

#[test]
fn for_installs_no_proxy_bypass_does_not_block_build() {
    let proxy = ProxyConfig {
        https_proxy: Some("http://proxy.example:8080".into()),
        http_proxy: None,
        no_proxy: Some(NoProxySetting::Bypass),
    };
    ThrottledClient::for_installs(&proxy, &TlsConfig::default(), &PerRegistryTls::default())
        .expect("bypass + proxy URL builds");
}

/// End-to-end check that `for_installs` actually routes HTTP traffic
/// through the configured proxy. We stand a `mockito` server up as an
/// upstream HTTP proxy: when a client is configured with `http_proxy =
/// <mockito_url>` and asked to fetch a different target URL, the request
/// arrives at the mockito server bearing the absolute-form URI in its
/// request line and the matching `Proxy-Authorization` header from the
/// percent-decoded userinfo.
#[tokio::test]
async fn mockito_integration_http_proxy_forwards_request_with_basic_auth() {
    let mut proxy_server = mockito::Server::new_async().await;
    // The mock matches *any* path because reqwest's HTTP-proxy mode
    // sends the request line with the absolute-form URI of the target
    // (RFC 9112 §3.2.2). We pin auth & method instead.
    let mock = proxy_server
        .mock("GET", mockito::Matcher::Any)
        .match_header("proxy-authorization", "Basic dXNlckBuYW1lOnBAc3M=")
        .with_status(200)
        .with_body("ok")
        .expect(1)
        .create_async()
        .await;

    let proxy_url = proxy_server.url();
    // `user@name:p@ss` percent-encoded → `user%40name:p%40ss`; the
    // network layer percent-decodes both halves to `user@name` and
    // `p@ss` and base64-encodes the pair as `dXNlckBuYW1lOnBAc3M=` —
    // the value the mock matches above.
    let with_auth = proxy_url.replacen("//", "//user%40name:p%40ss@", 1);
    let cfg = ProxyConfig { https_proxy: None, http_proxy: Some(with_auth), no_proxy: None };
    let client =
        ThrottledClient::for_installs(&cfg, &TlsConfig::default(), &PerRegistryTls::default())
            .expect("valid proxy");
    let guard = client.acquire().await;
    let resp = guard.get("http://target.example/anything").send().await.expect("proxied request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");
    mock.assert_async().await;
}

#[tokio::test]
async fn mockito_integration_no_proxy_bypasses_proxy() {
    // Sanity check the bypass path: with `NoProxySetting::Bypass`, the
    // client must not consult the proxy at all. We register the proxy
    // mock with `expect(0)` and rely on `mockito`'s drop-time assertion.
    let mut proxy_server = mockito::Server::new_async().await;
    let proxy_mock = proxy_server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .with_status(500)
        .create_async()
        .await;

    let mut target_server = mockito::Server::new_async().await;
    let target_path = "/direct";
    let target_mock = target_server
        .mock("GET", target_path)
        .expect(1)
        .with_status(200)
        .with_body("direct")
        .create_async()
        .await;

    let cfg = ProxyConfig {
        https_proxy: None,
        http_proxy: Some(proxy_server.url()),
        no_proxy: Some(NoProxySetting::Bypass),
    };
    let client =
        ThrottledClient::for_installs(&cfg, &TlsConfig::default(), &PerRegistryTls::default())
            .expect("valid proxy");
    let guard = client.acquire().await;
    let url = format!("{}{}", target_server.url(), target_path);
    let resp = guard.get(&url).send().await.expect("direct request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "direct");
    proxy_mock.assert_async().await;
    target_mock.assert_async().await;
}

// --- TLS / local-address tests ---

/// Minimal self-signed certificate used to assert `Certificate::from_pem`
/// accepts well-formed PEM. Lives at
/// `crates/network/tests/fixtures/test-ca.pem` rather than inline so
/// the base64-encoded body stays out of the typos linter's word
/// dictionary. Regenerate with:
///
/// ```text
/// openssl req -x509 -newkey rsa:2048 -nodes -days 36500 \
///     -subj '/CN=pacquet-test' -keyout /dev/null \
///     -out crates/network/tests/fixtures/test-ca.pem
/// ```
///
/// The private key is discarded — only the cert is committed so the
/// workspace doesn't carry real key material. Each regeneration
/// produces a different cert (fresh keypair, fresh serial); that's
/// fine because nothing pins a specific issuer / fingerprint, the
/// fixture only needs to be a valid X.509 PEM that
/// `Certificate::from_pem` accepts.
const TEST_CA_PEM: &str = include_str!("../tests/fixtures/test-ca.pem");

#[test]
fn for_installs_with_valid_ca_pem_builds() {
    let tls = TlsConfig { ca: vec![TEST_CA_PEM.to_string()], ..TlsConfig::default() };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("valid CA PEM builds");
}

#[test]
fn for_installs_with_multiple_ca_pems_builds() {
    // Same cert twice — exercises the `for` loop over `tls.ca`.
    let tls = TlsConfig {
        ca: vec![TEST_CA_PEM.to_string(), TEST_CA_PEM.to_string()],
        ..TlsConfig::default()
    };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("multiple CA PEMs build");
}

#[test]
fn for_installs_with_invalid_ca_pem_errors_with_index() {
    // First entry valid, second malformed — the index in the error
    // must point at the broken one so users with a multi-cert
    // `cafile` can find which entry failed.
    let tls = TlsConfig {
        ca: vec![TEST_CA_PEM.to_string(), "not a pem certificate".to_string()],
        ..TlsConfig::default()
    };
    let err =
        ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
            .expect_err("invalid CA must error");
    eprintln!("err={err:?}");
    match err {
        ForInstallsError::Tls(super::TlsError::InvalidCa { index, .. }) => assert_eq!(index, 1),
        other => panic!("expected Tls(InvalidCa {{ index: 1 }}), got {other:?}"),
    }
}

#[test]
fn for_installs_strict_ssl_false_relaxes_verification() {
    // `danger_accept_invalid_certs(true)` is a builder-level toggle —
    // we can't observe it directly without a self-signed-cert HTTPS
    // server, and mockito speaks plain HTTP only. Asserting the
    // client builds is the best we can do here; a live-traffic
    // integration test would need a TLS-capable mock server (e.g.
    // `wiremock` with rustls) and is left as a future enhancement.
    let tls = TlsConfig { strict_ssl: Some(false), ..TlsConfig::default() };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("strict-ssl=false builds");
}

#[test]
fn for_installs_strict_ssl_default_is_true() {
    // No explicit `strict_ssl` — `apply_tls` should leave the
    // builder's default cert verification untouched (which is the
    // same as strict_ssl=true). Asserting the client builds is the
    // best we can do without a server; the absence of
    // `danger_accept_invalid_certs(true)` is the contract.
    let tls = TlsConfig { strict_ssl: None, ..TlsConfig::default() };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("strict-ssl unset builds");
}

#[test]
fn for_installs_local_address_pinned() {
    use std::net::Ipv4Addr;
    let tls = TlsConfig { local_address: Some(Ipv4Addr::LOCALHOST.into()), ..TlsConfig::default() };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("local_address pinning builds");
}

#[test]
fn for_installs_with_malformed_client_identity_errors() {
    // PKCS#8 PEM parser rejects garbage — surfaces as
    // `InvalidClientIdentity`.
    let tls = TlsConfig {
        cert: Some("not a real cert".to_string()),
        key: Some("not a real key".to_string()),
        ..TlsConfig::default()
    };
    let err =
        ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
            .expect_err("malformed cert/key must error");
    eprintln!("err={err:?}");
    let is_invalid =
        matches!(err, ForInstallsError::Tls(super::TlsError::InvalidClientIdentity { .. }));
    assert!(is_invalid, "err={err:?}: expected Tls(InvalidClientIdentity)");
}

#[test]
fn for_installs_with_cert_but_no_key_skips_identity() {
    // Both must be set for the identity wiring to fire — a `cert`
    // without `key` is silently ignored (pnpm's undici plumbing has
    // the same "both or neither" expectation). The client must still
    // build cleanly.
    let tls = TlsConfig { cert: Some(TEST_CA_PEM.to_string()), key: None, ..TlsConfig::default() };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("cert without key builds (identity skipped)");
}

// --- Per-registry routing tests ---

#[test]
fn for_installs_builds_per_registry_clients() {
    // Two per-registry overrides — one per host — plus an empty
    // override that `from_map` drops. The constructor should
    // produce a client per non-empty override.
    use crate::RegistryTls;
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(
        "//reg-a.example.com/".to_string(),
        RegistryTls { ca: Some(TEST_CA_PEM.to_string()), ..RegistryTls::default() },
    );
    map.insert(
        "//reg-b.example.com/".to_string(),
        RegistryTls { ca: Some(TEST_CA_PEM.to_string()), ..RegistryTls::default() },
    );
    let per_registry = PerRegistryTls::from_map(map);
    ThrottledClient::for_installs(&ProxyConfig::default(), &TlsConfig::default(), &per_registry)
        .expect("per-registry config builds");
}

#[test]
fn for_installs_per_registry_invalid_ca_errors() {
    // A malformed per-registry CA must surface as `InvalidCa` at
    // build time, same as the top-level path. The `index` in the
    // error indexes into the merged CA list — which is exactly the
    // one-element vec carrying the scoped PEM, so `index == 0`.
    use crate::RegistryTls;
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(
        "//bad.example.com/".to_string(),
        RegistryTls { ca: Some("not a pem".to_string()), ..RegistryTls::default() },
    );
    let per_registry = PerRegistryTls::from_map(map);
    let err = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &per_registry,
    )
    .expect_err("must error");
    eprintln!("err={err:?}");
    let is_invalid_ca =
        matches!(err, ForInstallsError::Tls(super::TlsError::InvalidCa { index: 0, .. }));
    assert!(is_invalid_ca, "err={err:?}: expected Tls(InvalidCa {{ index: 0 }})");
}

#[tokio::test]
async fn acquire_for_url_routes_per_registry_then_falls_back() {
    // End-to-end: build a client with one per-registry override
    // (different `ca`) and verify `acquire_for_url` returns
    // *distinct* clients for URLs that match the override versus
    // URLs that don't. The semaphore is still shared, so the two
    // calls can interleave under concurrency.
    //
    // We can't compare `Client` instances directly — reqwest's
    // `Client` doesn't implement `PartialEq`. Use `Client::user_agent`
    // round-trip via the request builder? No — both share the
    // default builder. Instead, compare the underlying pointer:
    // `&Client` is what the guard derefs to, and two distinct
    // builds produce two distinct `Client` allocations.
    use crate::RegistryTls;
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(
        "//reg.example.com/".to_string(),
        RegistryTls { ca: Some(TEST_CA_PEM.to_string()), ..RegistryTls::default() },
    );
    let per_registry = PerRegistryTls::from_map(map);
    let throttled = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &per_registry,
    )
    .expect("valid");

    let scoped_guard = throttled.acquire_for_url("https://reg.example.com/pkg").await;
    let default_guard = throttled.acquire_for_url("https://other.example.org/pkg").await;
    let scoped_ptr: *const reqwest::Client = &*scoped_guard;
    let default_ptr: *const reqwest::Client = &*default_guard;
    assert_ne!(
        scoped_ptr, default_ptr,
        "scoped and default URLs must route through different reqwest clients",
    );
}

#[tokio::test]
async fn acquire_for_url_falls_back_to_default_when_no_overrides() {
    // The common case — no per-registry overrides at all. The lookup
    // short-circuits and `acquire_for_url` always returns the
    // default client.
    let throttled = ThrottledClient::new_for_installs();
    let a = throttled.acquire_for_url("https://example.com/").await;
    let b = throttled.acquire_for_url("https://other.example.org/").await;
    let a_ptr: *const reqwest::Client = &*a;
    let b_ptr: *const reqwest::Client = &*b;
    assert_eq!(a_ptr, b_ptr, "without overrides every URL should hit the default client");
}

// --- PKCS#1 / rustls regression tests ---

/// PKCS#1 client cert + key fixture. Generated with:
///
/// ```text
/// openssl genrsa -traditional -out crates/network/tests/fixtures/test-client-pkcs1.key 2048
/// openssl req -new -x509 -key crates/network/tests/fixtures/test-client-pkcs1.key \
///     -days 36500 -subj '/CN=pacquet-pkcs1-test' \
///     -out crates/network/tests/fixtures/test-client-pkcs1.crt
/// ```
///
/// The `-traditional` flag pins openssl to PKCS#1 (`-----BEGIN RSA
/// PRIVATE KEY-----`) instead of the default PKCS#8 — which is the
/// whole point of the regression test below. The cert and key are
/// self-signed and committed so the test stays deterministic.
const TEST_CLIENT_PKCS1_CERT: &str = include_str!("../tests/fixtures/test-client-pkcs1.crt");
const TEST_CLIENT_PKCS1_KEY: &str = include_str!("../tests/fixtures/test-client-pkcs1.key");

#[test]
fn for_installs_with_pkcs1_client_key_builds() {
    // The whole reason we switched reqwest's TLS backend from
    // native-tls to rustls: native-tls's `Identity::from_pkcs8_pem`
    // rejected `-----BEGIN RSA PRIVATE KEY-----`; rustls's
    // `Identity::from_pem` accepts PKCS#1, PKCS#8, and EC keys.
    // This test pins the new contract — if a future change reverts
    // the backend or otherwise narrows the accepted key formats,
    // this build will fail with a clear `InvalidClientIdentity`.
    let tls = TlsConfig {
        cert: Some(TEST_CLIENT_PKCS1_CERT.to_string()),
        key: Some(TEST_CLIENT_PKCS1_KEY.to_string()),
        ..TlsConfig::default()
    };
    ThrottledClient::for_installs(&ProxyConfig::default(), &tls, &PerRegistryTls::default())
        .expect("PKCS#1 client key + cert builds with rustls backend");
}
