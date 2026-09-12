use super::{
    Arc, AtomicUsize, CappedDnsResolver, Duration, ForInstallsError, NetworkSettings,
    NoProxyMatcher, NoProxySetting, NonZeroUsize, Ordering, PerRegistryTls, ProxyConfig,
    ProxyError, RecordingResolver, Semaphore, TEST_CA_PEM, TEST_CLIENT_PKCS1_CERT,
    TEST_CLIENT_PKCS1_KEY, ThrottledClient, TlsConfig, Url, list, parse_proxy_url, strip_userinfo,
};
use reqwest::dns::Resolve as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

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

/// Fetches through a client built the way installs build theirs, so the
/// request goes through the resolver `configure_dns` wires in. The
/// server listens on a loopback IP but is addressed as `localhost`, a
/// name the platform's `getaddrinfo` answers from the host's own tables
/// on every OS.
#[tokio::test]
async fn install_client_resolves_hostnames_through_the_system_resolver() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/by-hostname")
        .expect(1)
        .with_status(200)
        .with_body("resolved")
        .create_async()
        .await;
    let port = server.socket_address().port();

    let client = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("default install client builds");
    let guard = client.acquire().await;
    let resp = guard
        .get(format!("http://localhost:{port}/by-hostname"))
        .send()
        .await
        .expect("localhost resolves and connects");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "resolved");
    mock.assert_async().await;
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
        ("registry.npmjs.org.", true),
        ("evilnpmjs.org", false),
        ("org", false),
    ] {
        let got = matcher.matches_host(host);
        assert_eq!(got, expected, "host={host}: expected match={expected}, got={got}");
    }
}

#[test]
fn no_proxy_matcher_leading_dot_matches_subdomains() {
    let matcher = NoProxyMatcher::from(Some(&list(&[".npmjs.org"])));
    for (host, expected) in [
        ("npmjs.org", true),
        ("registry.npmjs.org", true),
        ("foo.bar.npmjs.org", true),
        ("evilnpmjs.org", false),
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
fn for_installs_with_empty_proxy_urls_treats_them_as_unset() {
    // pnpm/pnpm#13533: exporting `HTTP_PROXY=` disables the proxy, so an
    // empty value must not reach `parse_proxy_url`.
    let proxy = ProxyConfig {
        https_proxy: Some(String::new()),
        http_proxy: Some(String::new()),
        no_proxy: None,
    };
    ThrottledClient::for_installs(
        &proxy,
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("empty proxy values are treated as unset");
}

#[test]
fn for_installs_with_invalid_proxy_url_errors() {
    for proxy in [
        ProxyConfig { https_proxy: Some("://nonsense".into()), http_proxy: None, no_proxy: None },
        ProxyConfig { https_proxy: None, http_proxy: Some("://nonsense".into()), no_proxy: None },
    ] {
        let err = ThrottledClient::for_installs(
            &proxy,
            &TlsConfig::default(),
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
        )
        .expect_err("must error");
        eprintln!("proxy={proxy:?} err={err:?}");
        let is_invalid = matches!(err, ForInstallsError::Proxy(ProxyError::InvalidProxy { .. }));
        assert!(is_invalid, "err={err:?}: expected ForInstallsError::Proxy(InvalidProxy)");
    }
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
async fn no_redirect_client_returns_the_first_redirect_response() {
    let mut registry = mockito::Server::new_async().await;
    let start_mock = registry
        .mock("GET", "/start")
        .with_status(302)
        .with_header("location", "/final")
        .expect(1)
        .create_async()
        .await;
    let final_mock = registry
        .mock("GET", "/final")
        .with_status(200)
        .with_body("must not be fetched")
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}/start", registry.url());
    let client = ThrottledClient::default();

    let response = client
        .acquire_for_url_without_redirects_with_priority(&url, 0)
        .await
        .get(&url)
        .send()
        .await
        .expect("the redirect response itself is successful HTTP transport");

    assert_eq!(response.status(), 302);
    start_mock.assert_async().await;
    final_mock.assert_async().await;
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
        matches!(err, ForInstallsError::Tls(super::super::TlsError::InvalidClientIdentity { .. }));
    assert!(is_invalid, "err={err:?}: expected Tls(InvalidClientIdentity)");
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
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &per_registry,
        &NetworkSettings::default(),
    )
    .expect("per-registry config builds");
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

    let scoped_guard = throttled
        .acquire_for_url_without_redirects_with_priority("https://reg.example.com/pkg", 0)
        .await;
    let default_guard = throttled
        .acquire_for_url_without_redirects_with_priority("https://other.example.org/pkg", 0)
        .await;
    let scoped_ptr: *const reqwest::Client = &raw const *scoped_guard;
    let default_ptr: *const reqwest::Client = &raw const *default_guard;
    assert_ne!(
        scoped_ptr, default_ptr,
        "scoped and default URLs must route through different no-redirect clients",
    );
}

#[tokio::test]
async fn per_registry_route_selects_the_client_for_the_requested_redirect_mode() {
    // Pointer identity cannot see a routed pair whose two members
    // were built the wrong way round, so drive a real redirect
    // through both accessors instead.
    use crate::RegistryTls;
    use std::collections::HashMap;

    let mut registry = mockito::Server::new_async().await;
    let start_mock = registry
        .mock("GET", "/start")
        .with_status(302)
        .with_header("location", "/final")
        .expect(2)
        .create_async()
        .await;
    let final_mock = registry
        .mock("GET", "/final")
        .with_status(200)
        .with_body("followed")
        .expect(1)
        .create_async()
        .await;

    let mut map = HashMap::new();
    map.insert(
        format!("//{}/", registry.host_with_port()),
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

    let url = format!("{}/start", registry.url());
    // The statuses below hold for the default pair too, so pin the
    // routing first: a route map that lost the key would fall back
    // and still look green.
    {
        let routed_guard = throttled.acquire_for_url(&url).await;
        let unmatched_guard = throttled.acquire_for_url("https://other.example.org/pkg").await;
        let routed: *const reqwest::Client = &raw const *routed_guard;
        let unmatched: *const reqwest::Client = &raw const *unmatched_guard;
        assert_ne!(routed, unmatched, "the fixture URL must reach its own routed client");
    }

    let blocked = throttled
        .acquire_for_url_without_redirects_with_priority(&url, 0)
        .await
        .get(&url)
        .send()
        .await
        .expect("the redirect response itself is successful HTTP transport");
    assert_eq!(blocked.status(), 302, "the routed no-redirect client must not follow");

    let followed = throttled
        .acquire_for_url(&url)
        .await
        .get(&url)
        .send()
        .await
        .expect("the routed redirect-following client reaches the target");
    assert_eq!(followed.status(), 200, "the routed redirect-following client must follow");

    start_mock.assert_async().await;
    final_mock.assert_async().await;
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
