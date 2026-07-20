//! Tests for [`super`]'s proxy plumbing.
//!
//! Covers the proxy behaviors that don't require a real proxy listener:
//!
//! * `HTTP proxy` — per-URL routing, basic-auth decoding, scheme bypass.
//! * `SOCKS proxy` — routing decision (live-network case skipped).
//! * `noProxy` — reverse-dot-segment match, bypass-all literal.
//! * `Invalid proxy URL` — `ERR_PNPM_INVALID_PROXY`.
//!
//! The one HTTP integration test stands up a [`mockito`] server playing
//! the role of an HTTP proxy and asserts the request arrives with an
//! absolute-form URI and a decoded `Proxy-Authorization` header.

use super::{
    CappedDnsResolver, ForInstallsError, NetworkSettings, NoProxyMatcher, NoProxySetting,
    PerRegistryTls, ProxyConfig, ProxyError, ThrottledClient, TlsConfig, origin_of,
    parse_proxy_url,
};
use crate::proxy::{percent_decode_str, strip_userinfo};
use pacquet_testing_utils::env_guard::EnvGuard;
use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
};

struct RecordingResolver {
    active: Arc<AtomicUsize>,
    gate: Arc<Semaphore>,
    maximum_active: Arc<AtomicUsize>,
}

impl Resolve for RecordingResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        let active = Arc::clone(&self.active);
        let gate = Arc::clone(&self.gate);
        let maximum_active = Arc::clone(&self.maximum_active);
        Box::pin(async move {
            let active_count = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_active.fetch_max(active_count, Ordering::SeqCst);
            let _gate_permit =
                gate.acquire_owned().await.expect("test gate semaphore is never closed");
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(Box::new(std::iter::empty()) as Addrs)
        })
    }
}

#[tokio::test]
async fn capped_dns_resolver_limits_concurrency() {
    let active = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Semaphore::new(0));
    let maximum_active = Arc::new(AtomicUsize::new(0));
    let resolver = Arc::new(CappedDnsResolver::new(
        RecordingResolver {
            active: Arc::clone(&active),
            gate: Arc::clone(&gate),
            maximum_active: Arc::clone(&maximum_active),
        },
        NonZeroUsize::new(4).expect("four is non-zero"),
    ));
    let tasks = (0..8)
        .map(|_| {
            let resolver = Arc::clone(&resolver);
            tokio::spawn(async move {
                let _addresses = resolver
                    .resolve("registry.npmjs.org".parse().expect("valid DNS name"))
                    .await
                    .expect("recording resolver succeeds");
            })
        })
        .collect::<Vec<_>>();

    tokio::time::timeout(Duration::from_secs(1), async {
        while active.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("four resolutions should start");
    assert_eq!(maximum_active.load(Ordering::SeqCst), 4);

    gate.add_permits(8);
    for task in tasks {
        task.await.expect("resolution task succeeds");
    }
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(maximum_active.load(Ordering::SeqCst), 4);
}

fn list(entries: &[&str]) -> NoProxySetting {
    NoProxySetting::List(entries.iter().map(|s| (*s).to_string()).collect())
}

#[test]
fn no_proxy_matcher_reverse_dot_match() {
    let matcher = NoProxyMatcher::from(Some(&list(&["npmjs.org"])));
    // The matcher state is the same across every probe; logging it
    // once per test makes a failure diagnosable without rerunning.
    eprintln!("matcher={matcher:?}");
    for (host, expected) in [
        ("npmjs.org", true),
        ("registry.npmjs.org", true),
        ("foo.bar.npmjs.org", true),
        ("evilnpmjs.org", false),
        ("org", false),
    ] {
        let got = matcher.matches_host(host);
        assert_eq!(got, expected, "host={host}: expected match={expected}, got={got}");
    }
}

#[test]
fn no_proxy_matcher_empty_entries_never_match() {
    // Trailing/leading commas in `.npmrc` already get filtered in the
    // config layer's `parse_no_proxy`, but a malformed `List(vec![""])`
    // must still fail to match — defense in depth at the matcher.
    let matcher = NoProxyMatcher::from(Some(&list(&[""])));
    let got = matcher.matches_host("anything.example");
    assert!(!got, "matcher={matcher:?} host=anything.example expected miss, got match");
}

#[test]
fn no_proxy_matcher_multiple_entries() {
    let matcher = NoProxyMatcher::from(Some(&list(&["npmjs.org", "internal.example"])));
    eprintln!("matcher={matcher:?}");
    for (host, expected) in
        [("registry.npmjs.org", true), ("ci.internal.example", true), ("public.example", false)]
    {
        let got = matcher.matches_host(host);
        assert_eq!(got, expected, "host={host}: expected={expected}, got={got}");
    }
}

#[test]
fn no_proxy_bypass_short_circuits_every_host() {
    let matcher = NoProxyMatcher::from(Some(&NoProxySetting::Bypass));
    eprintln!("matcher={matcher:?}");
    for host in ["any.host", ""] {
        let got = matcher.matches_host(host);
        assert!(got, "host={host:?}: bypass must match every host, got miss");
    }
}

#[test]
fn no_proxy_none_matches_nothing() {
    let matcher = NoProxyMatcher::from(None);
    let got = matcher.matches_host("registry.npmjs.org");
    assert!(!got, "matcher={matcher:?}: None setting must never match");
}

#[test]
fn parse_proxy_url_auto_prefixes_missing_scheme() {
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
    // socks4, socks4a, and socks5 are honored. Routing happens
    // elsewhere; here we only assert the URL parses.
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
    // Diagnostic code is `ERR_PNPM_INVALID_PROXY`.
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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
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
    ThrottledClient::for_installs(
        &proxy,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("valid proxy URLs build");
}

#[test]
fn for_installs_with_invalid_proxy_url_errors() {
    let proxy =
        ProxyConfig { https_proxy: Some("://nonsense".into()), http_proxy: None, no_proxy: None };
    let err = ThrottledClient::for_installs(
        &proxy,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    ThrottledClient::for_installs(
        &proxy,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("socks proxy URL builds");
}

#[test]
fn for_installs_no_proxy_bypass_does_not_block_build() {
    let proxy = ProxyConfig {
        https_proxy: Some("http://proxy.example:8080".into()),
        http_proxy: None,
        no_proxy: Some(NoProxySetting::Bypass),
    };
    ThrottledClient::for_installs(
        &proxy,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    let client = ThrottledClient::for_installs(
        &cfg,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    let client = ThrottledClient::for_installs(
        &cfg,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("valid proxy");
    let guard = client.acquire().await;
    let url = format!("{}{}", target_server.url(), target_path);
    let resp = guard.get(&url).send().await.expect("direct request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "direct");
    proxy_mock.assert_async().await;
    target_mock.assert_async().await;
}

#[tokio::test]
async fn authorization_is_removed_on_cross_origin_redirect() {
    let mut target = mockito::Server::new_async().await;
    let target_mock = target
        .mock("GET", "/final")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body("ok")
        .expect(1)
        .create_async()
        .await;
    let mut registry = mockito::Server::new_async().await;
    let registry_mock = registry
        .mock("GET", "/start")
        .match_header("authorization", "Bearer 123")
        .with_status(302)
        .with_header("location", &format!("{}/final", target.url()))
        .expect(1)
        .create_async()
        .await;

    let client = ThrottledClient::default();
    let response = client
        .acquire()
        .await
        .get(format!("{}/start", registry.url()))
        .header("authorization", "Bearer 123")
        .send()
        .await
        .expect("follow cross-origin redirect");

    assert_eq!(response.status(), 200);
    registry_mock.assert_async().await;
    target_mock.assert_async().await;
}

#[tokio::test]
async fn authorization_is_retained_on_same_origin_redirect() {
    let mut registry = mockito::Server::new_async().await;
    let start_mock = registry
        .mock("GET", "/start")
        .match_header("authorization", "Bearer 123")
        .with_status(302)
        .with_header("location", "/final")
        .expect(1)
        .create_async()
        .await;
    let final_mock = registry
        .mock("GET", "/final")
        .match_header("authorization", "Bearer 123")
        .with_status(200)
        .with_body("ok")
        .expect(1)
        .create_async()
        .await;

    let client = ThrottledClient::default();
    let response = client
        .acquire()
        .await
        .get(format!("{}/start", registry.url()))
        .header("authorization", "Bearer 123")
        .send()
        .await
        .expect("follow same-origin redirect");

    assert_eq!(response.status(), 200);
    start_mock.assert_async().await;
    final_mock.assert_async().await;
}

#[tokio::test]
async fn https_target_uses_configured_proxy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
    let proxy_addr = listener.local_addr().expect("proxy address");
    let proxy = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept proxy connection");
        let mut request = vec![0; 1024];
        let size = stream.read(&mut request).await.expect("read CONNECT request");
        stream
            .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("reject tunnel after recording it");
        String::from_utf8(request[..size].to_vec()).expect("CONNECT request is ASCII")
    });
    let config = ProxyConfig {
        https_proxy: Some(format!("http://{proxy_addr}")),
        http_proxy: None,
        no_proxy: None,
    };
    let client = ThrottledClient::for_installs(
        &config,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("build HTTPS proxy client");

    client
        .acquire()
        .await
        .get("https://target.example/package")
        .send()
        .await
        .expect_err("the recording proxy rejects the tunnel");
    let connect = proxy.await.expect("proxy task");

    assert!(connect.starts_with("CONNECT target.example:443 HTTP/1.1\r\n"), "got {connect:?}");
}

#[tokio::test]
async fn socks5_proxy_connects_to_real_target() {
    let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind target");
    let target_addr = target_listener.local_addr().expect("target address");
    let target = tokio::spawn(async move {
        let (mut stream, _) = target_listener.accept().await.expect("accept target connection");
        let mut request = vec![0; 1024];
        let size = stream.read(&mut request).await.expect("read target request");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .expect("write target response");
        String::from_utf8(request[..size].to_vec()).expect("HTTP request is ASCII")
    });

    let socks_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind SOCKS5");
    let socks_addr = socks_listener.local_addr().expect("SOCKS5 address");
    let socks = tokio::spawn(async move {
        let (mut inbound, _) = socks_listener.accept().await.expect("accept SOCKS5 connection");
        let mut greeting = [0; 2];
        inbound.read_exact(&mut greeting).await.expect("read SOCKS5 greeting");
        assert_eq!(greeting[0], 5);
        let mut methods = vec![0; usize::from(greeting[1])];
        inbound.read_exact(&mut methods).await.expect("read SOCKS5 methods");
        inbound.write_all(&[5, 0]).await.expect("accept no-auth method");

        let mut request = [0; 4];
        inbound.read_exact(&mut request).await.expect("read SOCKS5 request");
        assert_eq!(&request[..3], &[5, 1, 0]);
        match request[3] {
            1 => {
                let mut address = [0; 4];
                inbound.read_exact(&mut address).await.expect("read IPv4 target");
            }
            3 => {
                let length = inbound.read_u8().await.expect("read domain length");
                let mut address = vec![0; usize::from(length)];
                inbound.read_exact(&mut address).await.expect("read domain target");
            }
            4 => {
                let mut address = [0; 16];
                inbound.read_exact(&mut address).await.expect("read IPv6 target");
            }
            atyp => panic!("unsupported SOCKS5 address type {atyp}"),
        }
        let port = inbound.read_u16().await.expect("read target port");
        assert_eq!(port, target_addr.port());
        let mut outbound =
            tokio::net::TcpStream::connect(target_addr).await.expect("connect target");
        inbound
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, (port >> 8) as u8, port as u8])
            .await
            .expect("accept SOCKS5 connect");
        tokio::io::copy_bidirectional(&mut inbound, &mut outbound)
            .await
            .expect("forward SOCKS5 traffic");
    });

    let config = ProxyConfig {
        https_proxy: None,
        http_proxy: Some(format!("socks5://{socks_addr}")),
        no_proxy: None,
    };
    let client = ThrottledClient::for_installs(
        &config,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("build SOCKS5 client");
    let response = client
        .acquire()
        .await
        .get(format!("http://{target_addr}/package"))
        .send()
        .await
        .expect("request through SOCKS5");

    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.expect("target body"), "ok");
    let request = target.await.expect("target task");
    assert!(request.starts_with("GET /package HTTP/1.1\r\n"), "got {request:?}");
    socks.await.expect("SOCKS5 task");
}

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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("valid CA PEM builds");
}

#[test]
fn for_installs_with_multiple_ca_pems_builds() {
    // Same cert twice — exercises the `for` loop over `tls.ca`.
    let tls = TlsConfig {
        ca: vec![TEST_CA_PEM.to_string(), TEST_CA_PEM.to_string()],
        ..TlsConfig::default()
    };
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    let err = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("strict-ssl unset builds");
}

#[test]
fn node_extra_ca_certs_is_loaded_and_failures_are_non_fatal() {
    // `EnvGuard` serializes env-mutating tests process-wide and restores
    // the prior value on drop — including on panic — so a failing
    // `.expect()` below can't leak `NODE_EXTRA_CA_CERTS` into a sibling
    // test. `for_installs` re-reads the var on each call.
    let env = EnvGuard::snapshot(["NODE_EXTRA_CA_CERTS"]);
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test-ca.pem");

    let build = || {
        ThrottledClient::for_installs(
            &ProxyConfig::default(),
            &TlsConfig::default(),
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
        )
    };

    // Empty value: nothing to add.
    env.set("NODE_EXTRA_CA_CERTS", "");
    assert!(super::load_node_extra_ca_certs().is_empty());

    // A valid PEM bundle parses into one trust root, and a client built
    // with it succeeds.
    env.set("NODE_EXTRA_CA_CERTS", fixture);
    assert_eq!(super::load_node_extra_ca_certs().len(), 1);
    build().expect("NODE_EXTRA_CA_CERTS pointing at a valid PEM builds");

    // A readable file that isn't valid PEM: ignored → empty.
    let bad =
        std::env::temp_dir().join(format!("pacquet-node-extra-ca-{}.pem", std::process::id()));
    std::fs::write(&bad, b"not a certificate").expect("write temp ca bundle");
    env.set("NODE_EXTRA_CA_CERTS", &bad);
    assert!(super::load_node_extra_ca_certs().is_empty());
    let _ = std::fs::remove_file(&bad);

    // A nonexistent file: unreadable, ignored → empty.
    env.set("NODE_EXTRA_CA_CERTS", "/pacquet/does-not-exist.pem");
    assert!(super::load_node_extra_ca_certs().is_empty());
    // `env` restores NODE_EXTRA_CA_CERTS on drop.
}

#[test]
fn for_installs_local_address_pinned() {
    use std::net::Ipv4Addr;
    let tls = TlsConfig { local_address: Some(Ipv4Addr::LOCALHOST.into()), ..TlsConfig::default() };
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    let err = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("cert without key builds (identity skipped)");
}

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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &per_registry,
        &NetworkSettings::default(),
    )
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
        &NetworkSettings::default(),
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
    // `Client` doesn't implement `PartialEq`. Compare the underlying
    // pointer instead: `&Client` is what the guard derefs to, and two
    // distinct builds produce two distinct `Client` allocations.
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
        &NetworkSettings::default(),
    )
    .expect("valid");

    let scoped_guard = throttled.acquire_for_url("https://reg.example.com/pkg").await;
    let default_guard = throttled.acquire_for_url("https://other.example.org/pkg").await;
    let scoped_ptr: *const reqwest::Client = &raw const *scoped_guard;
    let default_ptr: *const reqwest::Client = &raw const *default_guard;
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
    let permit_a = throttled.acquire_for_url("https://example.com/").await;
    let permit_b = throttled.acquire_for_url("https://other.example.org/").await;
    let a_ptr: *const reqwest::Client = &raw const *permit_a;
    let b_ptr: *const reqwest::Client = &raw const *permit_b;
    assert_eq!(a_ptr, b_ptr, "without overrides every URL should hit the default client");
}

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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("PKCS#1 client key + cert builds with rustls backend");
}

#[test]
fn for_installs_honors_custom_network_settings() {
    // A custom concurrency / timeout / user-agent must thread through
    // without error — the settings reach the semaphore and the reqwest
    // builder rather than being ignored.
    let settings = NetworkSettings {
        network_concurrency: 4,
        fetch_timeout: std::time::Duration::from_secs(5),
        user_agent: "pnpm/9.9.9 npm/? node/? darwin arm64".to_string(),
    };
    let client = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &settings,
    )
    .expect("custom network settings build");
    assert_eq!(client.semaphore.available_permits(), 4);
}

#[test]
fn for_installs_rejects_zero_network_concurrency() {
    // A zero-permit semaphore would hang every fetch; pnpm rejects the
    // same value, so `for_installs` must fail fast rather than deadlock.
    let settings = NetworkSettings { network_concurrency: 0, ..NetworkSettings::default() };
    let err = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &settings,
    )
    .expect_err("zero network concurrency must error");
    assert!(matches!(err, ForInstallsError::ZeroNetworkConcurrency), "got {err:?}");
}

#[test]
fn for_installs_falls_back_on_unencodable_user_agent() {
    // A user-agent containing a control character cannot be encoded as
    // an HTTP header value; the client must still build (falling back
    // to the default UA) rather than erroring.
    let settings =
        NetworkSettings { user_agent: "bad\nua".to_string(), ..NetworkSettings::default() };
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &settings,
    )
    .expect("unencodable user-agent falls back to default");
}

/// Pins the floor and cap of the default request-concurrency formula.
/// The floor exists because downloads are I/O-bound: deriving it from
/// the core count left low-core CI runners draining multi-hundred-
/// tarball installs 16 requests at a time without saturating a
/// low-latency registry.
#[test]
fn default_network_concurrency_stays_within_floor_and_cap() {
    let concurrency = super::default_network_concurrency();
    assert!((64..=96).contains(&concurrency), "got {concurrency}");
}

/// The blocked-redirect error must name only the origin, never the path or
/// query/fragment where a presigned-URL signature/token lives — the error can
/// reach a client.
#[test]
fn blocked_redirect_error_redacts_token() {
    let url = Url::parse("https://cdn.example:8443/asset.tgz?X-Amz-Signature=topsecret#frag")
        .expect("valid url");
    let message = super::BlockedRedirect(url).to_string();
    assert!(message.contains("https://cdn.example:8443"), "got: {message}");
    assert!(!message.contains("topsecret"), "token leaked: {message}");
    assert!(!message.contains("asset.tgz"), "path leaked: {message}");
    assert!(!message.contains("frag"), "fragment leaked: {message}");
}

/// A redirect to a host the guard rejects must fail the request before the
/// off-allowlist target is ever contacted — the redirect SSRF boundary.
#[tokio::test]
async fn redirect_guard_blocks_off_allowlist_redirect_target() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/pkg")
        .with_status(302)
        .with_header("location", "http://169.254.169.254/internal")
        .create_async()
        .await;

    // Allow only the entry server's own origin, never the redirect target.
    let allowed = format!("{}/", server.url());
    let client = ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
        url.as_str().starts_with(&allowed)
    });
    let guard = client.acquire().await;
    let result = guard.get(format!("{}/pkg", server.url())).send().await;

    redirect.assert_async().await;
    assert!(result.is_err(), "a redirect to an off-allowlist host must be blocked");
}

/// A redirect whose target the guard allows is followed normally, so an
/// allowlisted registry that legitimately redirects keeps working.
#[tokio::test]
async fn redirect_guard_follows_allowlisted_redirect_target() {
    let mut target = mockito::Server::new_async().await;
    let body = target.mock("GET", "/final").with_status(200).with_body("ok").create_async().await;
    let mut entry = mockito::Server::new_async().await;
    let redirect = entry
        .mock("GET", "/pkg")
        .with_status(302)
        .with_header("location", &format!("{}/final", target.url()))
        .create_async()
        .await;

    let entry_origin = format!("{}/", entry.url());
    let target_origin = format!("{}/", target.url());
    let client = ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
        let url = url.as_str();
        url.starts_with(&entry_origin) || url.starts_with(&target_origin)
    });
    let guard = client.acquire().await;
    let resp = guard.get(format!("{}/pkg", entry.url())).send().await.expect("redirect followed");

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");
    redirect.assert_async().await;
    body.assert_async().await;
}

#[test]
fn origin_of_extracts_scheme_host_and_port() {
    assert_eq!(
        origin_of("https://registry.npmjs.org/is-odd").as_deref(),
        Some("https://registry.npmjs.org"),
    );
    // A non-default port is part of the origin key.
    assert_eq!(
        origin_of("http://localhost:4873/is-odd/-/is-odd-3.0.1.tgz").as_deref(),
        Some("http://localhost:4873"),
    );
    // An explicit scheme-default port normalizes to the same origin as the
    // implicit form, so it cannot fragment the per-origin socket cap.
    assert_eq!(origin_of("https://host/a"), origin_of("https://host:443/a"));
    assert_eq!(origin_of("http://host/a"), origin_of("http://host:80/a"));
    // A non-default port stays distinct.
    assert_ne!(origin_of("https://host/a"), origin_of("https://host:8443/a"));
    // Same host over http vs https are distinct origins.
    assert_ne!(origin_of("http://example.com/a"), origin_of("https://example.com/a"));
    assert_eq!(origin_of("not a url"), None);
}

/// `maxSockets` caps concurrent in-flight requests to a single origin: a
/// second request to the same origin blocks while the first guard is held,
/// but a request to a different origin is unaffected.
#[tokio::test]
async fn max_sockets_caps_concurrent_sockets_per_origin() {
    use std::time::Duration;

    let client = ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1));
    let held = client.acquire_for_url("https://registry.example.com/a").await;

    // A second socket to the same origin must wait for `held` to drop.
    let blocked = tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://registry.example.com/b"),
    )
    .await;
    assert!(blocked.is_err(), "second socket to the same origin should block under maxSockets=1");

    // A different origin has its own budget and is not blocked.
    tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://other.example.com/a"),
    )
    .await
    .expect("a different origin should not be blocked");

    // Releasing the first guard frees the origin's single slot.
    drop(held);
    tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://registry.example.com/c"),
    )
    .await
    .expect("the origin's slot should be free after the first guard drops");
}

/// Without a `maxSockets` cap, many concurrent requests to one origin all
/// acquire immediately (bounded only by the global concurrency semaphore).
#[tokio::test]
async fn no_max_sockets_leaves_per_origin_uncapped() {
    use std::time::Duration;

    let client = ThrottledClient::new_for_installs();
    let _g1 = client.acquire_for_url("https://registry.example.com/a").await;
    tokio::time::timeout(
        Duration::from_millis(150),
        client.acquire_for_url("https://registry.example.com/b"),
    )
    .await
    .expect("a second socket to the same origin should not block without maxSockets");
}
