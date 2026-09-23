use super::TEST_CA_PEM;
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
use super::{EnvGuard, NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig};

// `SSL_CERT_FILE` alone switches `rustls-native-certs` to env-only
// loading, so pointing it at an empty file is a portable stand-in for a
// machine whose system trust store holds nothing — the nixpkgs sandbox
// of pnpm/pnpm#13588. Only the Linux/BSD platform verifier reads that
// variable; the Apple and Windows ones go to the OS keychain.
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
#[test]
fn for_installs_falls_back_to_bundled_roots_without_a_system_trust_store() {
    let env = EnvGuard::snapshot(["SSL_CERT_FILE", "SSL_CERT_DIR", "NODE_EXTRA_CA_CERTS"]);
    let empty_bundle =
        std::env::temp_dir().join(format!("pacquet-empty-ca-{}.pem", std::process::id()));
    std::fs::write(&empty_bundle, b"").expect("write empty ca bundle");
    env.set("SSL_CERT_FILE", &empty_bundle);
    env.set("SSL_CERT_DIR", &empty_bundle);
    // An extra root of any kind keeps the platform verifier alive, so
    // the fallback would go untested with the developer's own value.
    env.set("NODE_EXTRA_CA_CERTS", "");

    assert!(
        reqwest::Client::builder().build().is_err(),
        "precondition: the platform verifier must fail without a system trust store",
    );

    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("bundled Mozilla roots stand in for the missing system trust store");

    // A configured `ca` is itself enough to keep the platform verifier
    // constructible, so a user with custom roots never reaches the
    // fallback — their roots cannot be displaced by it.
    let ca =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test-ca.pem"))
            .expect("read test ca fixture");
    assert!(
        reqwest::Client::builder()
            .add_root_certificate(
                reqwest::Certificate::from_pem(ca.as_bytes()).expect("parse test ca fixture")
            )
            .build()
            .is_ok(),
        "a custom CA root keeps the platform verifier constructible",
    );
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig { ca: vec![ca], ..TlsConfig::default() },
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("a configured ca builds without a system trust store");

    let _ = std::fs::remove_file(&empty_bundle);
}

#[test]
fn a_corrupt_block_does_not_discard_the_rest_of_a_ca_bundle() {
    // A per-registry `:ca` / `:cafile` arrives as one buffer, so a
    // single corrupt block must not cost the registry every custom
    // root in it.
    const CORRUPT: &str = "-----BEGIN CERTIFICATE-----\nnot-base64!!!\n-----END CERTIFICATE-----";
    let bundle = format!("{TEST_CA_PEM}\n{CORRUPT}\n{TEST_CA_PEM}\n");
    assert_eq!(crate::certificates::parse_ca_bundle(bundle.as_bytes()).len(), 2);
    assert_eq!(crate::certificates::parse_ca_bundle(CORRUPT.as_bytes()).len(), 0);
    assert_eq!(crate::certificates::parse_ca_bundle(TEST_CA_PEM.as_bytes()).len(), 1);
}

#[cfg(target_vendor = "apple")]
#[test]
fn platform_verifier_detects_unreachable_trustd_under_sandbox() {
    use super::{NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig};

    if std::env::var("PNPM_TEST_DENIED_TRUSTD").is_ok() {
        assert!(!crate::certificates::is_platform_verifier_available());
        let client = ThrottledClient::for_installs(
            &ProxyConfig::default(),
            &TlsConfig::default(),
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
        );
        assert!(client.is_ok(), "client should build with bundled roots fallback under sandbox");

        let client_with_ca = ThrottledClient::for_installs(
            &ProxyConfig::default(),
            &TlsConfig { ca: vec![TEST_CA_PEM.to_string()], ..TlsConfig::default() },
            &PerRegistryTls::default(),
            &NetworkSettings::default(),
        );
        assert!(client_with_ca.is_ok(), "client with custom ca should build under sandbox");
        return;
    }

    assert!(crate::certificates::is_platform_verifier_available());

    let exe = std::env::current_exe().expect("current test executable path");
    let output = std::process::Command::new("sandbox-exec")
        .args([
            "-p",
            r#"(version 1)(allow default)(deny mach-lookup (global-name "com.apple.trustd.agent"))"#,
        ])
        .arg(&exe)
        .arg("--exact")
        .arg("tests::manifests::platform_verifier_detects_unreachable_trustd_under_sandbox")
        .arg("--nocapture")
        .env("PNPM_TEST_DENIED_TRUSTD", "1")
        .output()
        .expect("sandbox-exec execution must succeed");

    assert!(
        output.status.success(),
        "sandbox test failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
