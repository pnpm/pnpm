use super::{
    Arc, AuthHeaders, Duration, EnvGuard, NetworkSettings, PerRegistryTls, ProxyConfig,
    TEST_CA_PEM, TEST_CLIENT_PKCS1_CERT, TEST_CLIENT_PKCS1_KEY, ThrottledClient, TlsConfig, Url,
    nerf_dart,
};

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
async fn secure_auth_is_re_evaluated_for_each_redirect_target() {
    let mut target = mockito::Server::new_async().await;
    let target_mock = target
        .mock("GET", "/final")
        .match_header("accept", "application/vnd.pypi.simple.v1+json")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body("ok")
        .expect(1)
        .create_async()
        .await;
    let mut registry = mockito::Server::new_async().await;
    let start_mock = registry
        .mock("GET", "/start")
        .match_header("accept", "application/vnd.pypi.simple.v1+json")
        .match_header("authorization", "Bearer registry-token")
        .with_status(302)
        .with_header("location", "/same-origin")
        .expect(1)
        .create_async()
        .await;
    let same_origin_mock = registry
        .mock("GET", "/same-origin")
        .match_header("authorization", "Bearer registry-token")
        .with_status(302)
        .with_header("location", &format!("{}/final", target.url()))
        .expect(1)
        .create_async()
        .await;
    let auth_headers = AuthHeaders::from_creds_map([(
        nerf_dart(&registry.url()),
        "Bearer registry-token".to_string(),
    )]);

    let response = ThrottledClient::default()
        .get_bytes_with_secure_auth_and_accept(
            &format!("{}/start", registry.url()),
            &auth_headers,
            Some("application/vnd.pypi.simple.v1+json"),
        )
        .await
        .expect("follow redirects with per-target authentication");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"ok");
    assert_eq!(response.url, format!("{}/final", target.url()));
    start_mock.assert_async().await;
    same_origin_mock.assert_async().await;
    target_mock.assert_async().await;
}

// Regression for <https://github.com/pnpm/pnpm/issues/14646>: an
// unreadable `ca` entry contributes no trust anchor and the client
// still builds, the way Node ignores CA material it cannot parse.
#[test]
fn for_installs_ignores_ca_entries_that_carry_no_certificate() {
    let unreadable = ["", "${CORP_CA}", "not a pem certificate"];
    for pem in unreadable {
        assert!(
            crate::certificates::parse_ca_bundle(pem.as_bytes()).is_empty(),
            "{pem:?} parsed as a cert",
        );
    }
    let mut ca: Vec<String> = unreadable.iter().map(|pem| (*pem).to_string()).collect();
    ca.push(TEST_CA_PEM.to_string());
    // The valid entry must still reach the trust store — dropping it
    // alongside its unreadable neighbours would break the install a
    // different way.
    assert_eq!(
        ca.iter().flat_map(|pem| crate::certificates::parse_ca_bundle(pem.as_bytes())).count(),
        1,
    );
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig { ca, ..TlsConfig::default() },
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("unreadable CA entries are dropped, not fatal");
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
    assert!(super::super::load_node_extra_ca_certs().is_empty());

    // A valid PEM bundle parses into one trust root, and a client built
    // with it succeeds.
    env.set("NODE_EXTRA_CA_CERTS", fixture);
    assert_eq!(super::super::load_node_extra_ca_certs().len(), 1);
    build().expect("NODE_EXTRA_CA_CERTS pointing at a valid PEM builds");

    // A readable file that isn't valid PEM: ignored → empty.
    let bad =
        std::env::temp_dir().join(format!("pacquet-node-extra-ca-{}.pem", std::process::id()));
    std::fs::write(&bad, b"not a certificate").expect("write temp ca bundle");
    env.set("NODE_EXTRA_CA_CERTS", &bad);
    assert!(super::super::load_node_extra_ca_certs().is_empty());
    let _ = std::fs::remove_file(&bad);

    // A nonexistent file: unreadable, ignored → empty.
    env.set("NODE_EXTRA_CA_CERTS", "/pacquet/does-not-exist.pem");
    assert!(super::super::load_node_extra_ca_certs().is_empty());
    // `env` restores NODE_EXTRA_CA_CERTS on drop.
}

#[tokio::test]
async fn default_tls_rejects_an_untrusted_certificate_without_panicking() {
    use rustls::{ServerConfig, ServerConnection, pki_types::pem::PemObject};
    use std::net::TcpListener;

    let env = EnvGuard::snapshot(["NODE_EXTRA_CA_CERTS"]);
    for extra_ca in ["", "/pacquet/does-not-exist.pem"] {
        env.set("NODE_EXTRA_CA_CERTS", extra_ca);
        let client = ThrottledClient::for_installs(
            &ProxyConfig::default(),
            &TlsConfig::default(),
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
        )
        .expect("build client without a JVM or extra CA certificates");
        let cert = rustls::pki_types::CertificateDer::from_pem_slice(include_bytes!(
            "../../tests/fixtures/test-client-pkcs1.crt"
        ))
        .expect("parse server certificate");
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(include_bytes!(
            "../../tests/fixtures/test-client-pkcs1.key"
        ))
        .expect("parse server key");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("configure untrusted TLS server");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS server");
        let address = listener.local_addr().expect("TLS server address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept TLS connection");
            stream.set_read_timeout(Some(Duration::from_secs(10))).expect("set read timeout");
            stream.set_write_timeout(Some(Duration::from_secs(10))).expect("set write timeout");
            ServerConnection::new(Arc::new(config))
                .expect("create TLS connection")
                .complete_io(&mut stream)
                .expect_err("client rejects the untrusted server certificate");
        });

        let error = client
            .acquire()
            .await
            .get(format!("https://{address}/"))
            .send()
            .await
            .expect_err("untrusted certificate must fail verification");
        eprintln!("TLS error: {error:?}");
        assert!(error.is_connect(), "expected a TLS connection error: {error:?}");
        server.join().expect("TLS server thread");
    }
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
fn for_installs_does_not_retain_per_registry_tls_material() {
    use crate::RegistryTls;
    use std::collections::HashMap;
    const PRIVATE_KEY_MARKER: &str = "registry-private-key-marker";

    let mut map = HashMap::new();
    map.insert(
        "//reg.example.com/".to_string(),
        RegistryTls { key: Some(PRIVATE_KEY_MARKER.to_string()), ..RegistryTls::default() },
    );
    let per_registry = PerRegistryTls::from_map(map);
    let client = ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &per_registry,
        &NetworkSettings::default(),
    )
    .expect("per-registry config builds");

    let debug = format!("{client:?}");
    assert!(!debug.contains(PRIVATE_KEY_MARKER), "finished client retained TLS key: {debug}");
}

#[test]
fn for_installs_ignores_a_per_registry_ca_that_carries_no_certificate() {
    // Same tolerance as the top-level path: the override contributes
    // no trust anchor and the client still builds.
    use crate::RegistryTls;
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(
        "//bad.example.com/".to_string(),
        RegistryTls { ca: Some("not a pem".to_string()), ..RegistryTls::default() },
    );
    let per_registry = PerRegistryTls::from_map(map);
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &per_registry,
        &NetworkSettings::default(),
    )
    .expect("unreadable per-registry CA is dropped, not fatal");
}

#[test]
fn a_blank_scoped_cert_shadows_the_top_level_identity() {
    // pnpm spreads the per-registry entry over the top-level one, so a
    // blank `//reg/:cert=` overrides rather than falls back. Letting
    // it fall back would pair the top-level certificate with the
    // scoped key and send an identity the user never configured for
    // that registry.
    use crate::RegistryTls;
    let top = TlsConfig {
        cert: Some(TEST_CLIENT_PKCS1_CERT.to_string()),
        key: Some(TEST_CLIENT_PKCS1_KEY.to_string()),
        ..TlsConfig::default()
    };
    let scoped = RegistryTls { cert: Some(String::new()), ..RegistryTls::default() };
    assert_eq!(super::super::merge_tls(&top, &scoped).cert.as_deref(), Some(""));
}

/// The blocked-redirect error must name only the origin, never the path or
/// query/fragment where a presigned-URL signature/token lives — the error can
/// reach a client.
#[test]
fn blocked_redirect_error_redacts_token() {
    let url = Url::parse("https://cdn.example:8443/asset.tgz?X-Amz-Signature=topsecret#frag")
        .expect("valid url");
    let message = crate::client_builder::BlockedRedirect(url).to_string();
    assert!(message.contains("https://cdn.example:8443"), "got: {message}");
    assert!(!message.contains("topsecret"), "token leaked: {message}");
    assert!(!message.contains("asset.tgz"), "path leaked: {message}");
    assert!(!message.contains("frag"), "fragment leaked: {message}");
}
