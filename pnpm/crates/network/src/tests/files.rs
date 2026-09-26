use super::{
    NetworkSettings, PerRegistryTls, ProxyConfig, TEST_CLIENT_PKCS1_KEY, ThrottledClient, TlsConfig,
};

#[test]
fn for_installs_ignores_a_blank_client_identity() {
    // `cert=` / `key=` lines with nothing after them read as unset to
    // pnpm, which checks them for truthiness before handing them to
    // undici. Pairing a blank `cert` with a real `key` must not build
    // an identity rustls then rejects.
    let tls = TlsConfig {
        cert: Some("   ".to_string()),
        key: Some(TEST_CLIENT_PKCS1_KEY.to_string()),
        ..TlsConfig::default()
    };
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &tls,
        &PerRegistryTls::default(),
        &NetworkSettings::default(),
    )
    .expect("a blank cert leaves the identity unset");
}
