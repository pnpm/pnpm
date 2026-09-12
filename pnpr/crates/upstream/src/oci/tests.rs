use super::{oci_download_allowed, parse_challenge, token_realm_allowed};
use reqwest::Url;

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
}

#[tokio::test]
async fn negotiates_pull_scope_and_reuses_token_without_forwarding_client_scope() {
    use crate::{FetchOutcome, Upstream};
    use pnpr_config::UpstreamConfig;
    use reqwest::header::HeaderMap;
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
    use crate::Upstream;
    use pnpr_config::UpstreamConfig;
    use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
    let mut server = mockito::Server::new_async().await;
    let mut attacker = mockito::Server::new_async().await;
    let stolen = attacker.mock("GET", "/token").expect(0).create_async().await;
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
