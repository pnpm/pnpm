use super::{
    NetworkSettings, PerRegistryTls, ProxyConfig, ThrottledClient, TlsConfig, bundled_root_certs,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

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

#[tokio::test]
async fn from_clients_uses_the_supplied_no_redirect_configuration() {
    let mut registry = mockito::Server::new_async().await;
    let start_mock = registry
        .mock("GET", "/start")
        .match_header("user-agent", "strict-client")
        .with_status(302)
        .with_header("location", "/final")
        .expect(1)
        .create_async()
        .await;
    let final_mock = registry.mock("GET", "/final").expect(0).create_async().await;
    // Bundled roots only: a sibling test may have pointed `SSL_CERT_FILE` at
    // an empty bundle, which makes a platform-verifier client unbuildable.
    let client = reqwest::Client::builder()
        .tls_certs_only(bundled_root_certs().iter().cloned())
        .user_agent("ordinary-client")
        .build()
        .expect("build ordinary client");
    let client_without_redirects = reqwest::Client::builder()
        .tls_certs_only(bundled_root_certs().iter().cloned())
        .user_agent("strict-client")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build strict client");
    let client = ThrottledClient::from_clients(client, client_without_redirects);
    let url = format!("{}/start", registry.url());

    let response = client
        .acquire_for_url_without_redirects_with_priority(&url, 0)
        .await
        .get(&url)
        .send()
        .await
        .expect("strict client returns the redirect response");

    assert_eq!(response.status(), 302);
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

#[test]
fn for_installs_honors_custom_network_settings() {
    // Custom concurrency, timeout, warning thresholds, and user-agent must thread through
    // without error — the settings reach the semaphore and the reqwest
    // builder rather than being ignored.
    let settings = NetworkSettings {
        network_concurrency: 4,
        fetch_timeout: std::time::Duration::from_secs(5),
        fetch_warn_timeout: std::time::Duration::from_secs(2),
        fetch_min_speed_ki_bps: 75,
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
    assert_eq!(client.fetch_warn_timeout(), std::time::Duration::from_secs(2));
    assert_eq!(client.fetch_min_speed_ki_bps(), 75);
}
