use super::{oci_download_allowed, parse_challenge, token_realm_allowed};
use crate::{FetchOutcome, Upstream};
use pnpr_config::UpstreamConfig;
use reqwest::{Url, header::HeaderMap};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[test]
fn parses_pull_challenges_and_rejects_ambiguous_realms() {
    let challenge = parse_challenge(r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/alpine:pull,push""#).unwrap();
    assert_eq!(challenge.realm, "https://auth.docker.io/token");
    assert_eq!(challenge.service, "registry.docker.io");
    assert!(parse_challenge(r#"Bearer realm="https://a",realm="https://b""#).is_none());
}

#[test]
fn only_trusts_origin_specific_token_and_download_hosts() {
    let hub = Url::parse("https://registry-1.docker.io/").unwrap();
    let ghcr = Url::parse("https://ghcr.io/").unwrap();
    let auth = Url::parse("https://auth.docker.io/token").unwrap();
    assert!(token_realm_allowed(&hub, &auth));
    assert!(!token_realm_allowed(&ghcr, &auth));
    assert!(!token_realm_allowed(&hub, &Url::parse("http://auth.docker.io/token").unwrap()));
    let cdn = Url::parse("https://production.cloudflare.docker.com/layer").unwrap();
    assert!(oci_download_allowed(&hub, &cdn));
    assert!(!oci_download_allowed(&ghcr, &cdn));
    assert!(!oci_download_allowed(&hub, &Url::parse("http://127.0.0.1/layer").unwrap()));
    let quay = Url::parse("https://quay.io/").unwrap();
    for host in ["cdn", "cdn01", "cdn02", "cdn03", "cdn04", "cdn05", "cdn06"] {
        let quay_cdn = Url::parse(&format!("https://{host}.quay.io/layer")).unwrap();
        assert!(oci_download_allowed(&quay, &quay_cdn), "{quay_cdn}");
        assert!(!oci_download_allowed(&hub, &quay_cdn), "{quay_cdn}");
    }
    assert!(!oci_download_allowed(&quay, &Url::parse("https://cdn07.quay.io/layer").unwrap()));
    assert!(!oci_download_allowed(&quay, &Url::parse("http://cdn01.quay.io/layer").unwrap()));
}

#[tokio::test]
async fn negotiates_pull_scope_and_reuses_token_without_forwarding_client_scope() {
    let mut server = mockito::Server::new_async().await;
    let challenge = server.mock("GET", "/v2/acme/app/manifests/latest")
        .match_header("authorization", mockito::Matcher::Missing).with_status(401)
        .with_header("www-authenticate", &format!(r#"Bearer realm="{}/token",service="registry",scope="repository:other/app:pull,push""#, server.url()))
        .expect(1).create_async().await;
    let tokens = server
        .mock("GET", "/token")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("service".into(), "registry".into()),
            mockito::Matcher::UrlEncoded("scope".into(), "repository:acme/app:pull".into()),
        ]))
        .with_body(r#"{"token":"scoped","expires_in":300}"#)
        .expect(1)
        .create_async()
        .await;
    let authorized = server
        .mock("GET", "/v2/acme/app/manifests/latest")
        .match_header("authorization", "Bearer scoped")
        .with_body("manifest")
        .expect(2)
        .create_async()
        .await;
    let upstream = Upstream::new(
        "registry",
        &UpstreamConfig::with_defaults(format!("{}/", server.url()), HeaderMap::new()),
    );
    for _ in 0..2 {
        let FetchOutcome::Ok(response) =
            upstream.fetch_oci("acme/app", "manifests/latest", "application/json").await.unwrap()
        else {
            panic!("expected manifest")
        };
        assert_eq!(response.bytes().await.unwrap(), "manifest");
    }
    challenge.assert_async().await;
    tokens.assert_async().await;
    authorized.assert_async().await;
}

#[tokio::test]
async fn rejects_an_untrusted_token_realm_without_contacting_it() {
    use reqwest::header::{AUTHORIZATION, HeaderValue};
    let mut server = mockito::Server::new_async().await;
    let mut attacker = mockito::Server::new_async().await;
    let stolen = attacker
        .mock("GET", "/token")
        .expect(0)
        .create_async()
        .await;
    let challenge = server
        .mock("GET", "/v2/acme/app/manifests/latest")
        .with_status(401)
        .with_header(
            "www-authenticate",
            &format!(r#"Bearer realm="{}/token",service="registry""#, attacker.url()),
        )
        .create_async()
        .await;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic secret"));
    let upstream = Upstream::new(
        "registry",
        &UpstreamConfig::with_defaults(format!("{}/", server.url()), headers),
    );
    assert!(upstream.fetch_oci("acme/app", "manifests/latest", "application/json").await.is_err());
    challenge.assert_async().await;
    stolen.assert_async().await;
}

/// An OCI registry that answers the anonymous blob request with a 401 after
/// `challenge_delay`, issues a token after `token_delay`, and then serves the
/// blob.
async fn spawn_token_registry(
    challenge_delay: Duration,
    token_delay: Duration,
    token_body_delay: Duration,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let realm = format!("{base}/token");
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let realm = realm.clone();
            tokio::spawn(async move {
                let mut request = vec![0u8; 4096];
                let read = socket.read(&mut request).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]).to_lowercase();
                let response = if request.starts_with("get /token") {
                    tokio::time::sleep(token_delay).await;
                    let body = r#"{"token":"pull","expires_in":300}"#;
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len(),
                    );
                    let _ = socket.write_all(headers.as_bytes()).await;
                    tokio::time::sleep(token_body_delay).await;
                    body.to_string()
                } else if request.contains("authorization: bearer pull") {
                    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nblob"
                        .to_string()
                } else {
                    tokio::time::sleep(challenge_delay).await;
                    format!(
                        "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer realm=\"{realm}\",service=\"registry\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                };
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    base
}

fn token_upstream(base: &str, timeout: Duration) -> Upstream {
    let mut config = UpstreamConfig::with_defaults(format!("{base}/"), HeaderMap::new());
    config.requests.timeout = timeout;
    let mut upstream = Upstream::new("registry", &config);
    upstream.http.client = std::sync::Arc::new(
        pnpm_network::ThrottledClient::new_for_installs().with_max_sockets_per_host(Some(1)),
    );
    upstream
}

#[tokio::test]
async fn waiting_for_the_token_permit_does_not_spend_the_blob_budget() {
    let base =
        spawn_token_registry(Duration::from_millis(150), Duration::ZERO, Duration::ZERO).await;
    let upstream = token_upstream(&base, Duration::from_millis(250));
    // Queues behind the blob request for the only permit, so it holds the
    // permit while the token request waits for it.
    let hold_permit = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let held = upstream.http.client.acquire_for_url(&base).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        drop(held);
    };
    let fetch = upstream.fetch_oci("acme/app", "blobs/sha256:abc", "*/*");
    let (result, ()) = tokio::join!(fetch, hold_permit);
    let FetchOutcome::Ok(response) = result.unwrap() else { panic!("expected the blob") };
    assert_eq!(response.bytes().await.unwrap(), "blob");
}

#[tokio::test]
async fn token_negotiation_is_bounded_by_the_remaining_budget() {
    let base =
        spawn_token_registry(Duration::from_millis(400), Duration::from_secs(5), Duration::ZERO)
            .await;
    let upstream = token_upstream(&base, Duration::from_millis(500));
    let started = std::time::Instant::now();
    let result = upstream.fetch_oci("acme/app", "blobs/sha256:abc", "*/*").await;
    assert!(result.is_err(), "a stalled token endpoint fails the fetch");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_millis(800), "took {elapsed:?}, past the 500ms budget");
}

#[tokio::test]
async fn a_token_timeout_counts_against_the_breaker() {
    let base = spawn_token_registry(Duration::ZERO, Duration::from_secs(5), Duration::ZERO).await;
    let mut config = UpstreamConfig::with_defaults(format!("{base}/"), HeaderMap::new());
    config.requests.timeout = Duration::from_millis(200);
    config.requests.max_fails = 1;
    let upstream = Upstream::new("registry", &config);
    let first = upstream.fetch_oci("acme/app", "blobs/sha256:abc", "*/*").await;
    assert!(
        matches!(first, Err(pnpr_error::RegistryError::Upstream { ref source, .. }) if source.is_timeout()),
        "got {:?}",
        first.err(),
    );
    let second = upstream.fetch_oci("acme/app", "blobs/sha256:abc", "*/*").await;
    assert!(
        matches!(second, Err(pnpr_error::RegistryError::UpstreamUnavailable { .. })),
        "got {:?}",
        second.err(),
    );
}

#[tokio::test]
async fn token_body_timeouts_do_not_open_the_circuit() {
    let base = spawn_token_registry(Duration::ZERO, Duration::ZERO, Duration::from_secs(5)).await;
    let mut config = UpstreamConfig::with_defaults(format!("{base}/"), HeaderMap::new());
    config.requests.timeout = Duration::from_millis(200);
    config.requests.max_fails = 1;
    let upstream = Upstream::new("registry", &config);
    for _ in 0..2 {
        let result = upstream.fetch_oci("example/image", "blobs/sha256:abc", "*/*").await;
        assert!(
            matches!(result, Err(pnpr_error::RegistryError::Upstream { source, .. }) if source.is_timeout()),
        );
        assert!(upstream.ensure_available().is_ok());
    }
}
