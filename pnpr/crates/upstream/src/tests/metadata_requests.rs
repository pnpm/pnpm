use super::{
    CacheValidators, CanonicalPackageName, FetchOutcome, HeaderMap, PackumentFetch, RegistryError,
    UPSTREAM_ERROR_BODY_LIMIT, auth_and_custom_headers, breaking_upstream, json, upstream,
};

#[tokio::test]
async fn fetch_packument_forwards_configured_headers() {
    let mut server = mockito::Server::new_async().await;
    // The mock only matches when both headers are present, so an
    // `Ok` outcome proves they rode along on the request.
    let mock = server
        .mock("GET", "/foo")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), auth_and_custom_headers());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::Modified(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_tarball_response_forwards_configured_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body("tarball-bytes")
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), auth_and_custom_headers());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_tarball_response(&name, "foo-1.0.0.tgz").await.unwrap();

    assert!(matches!(outcome, FetchOutcome::Ok(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_revision_tarball_rejects_redirects_and_forwards_headers() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/-/tarballs/sha512/digest")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .with_body("x".repeat(UPSTREAM_ERROR_BODY_LIMIT + 1))
        .expect(1)
        .create_async()
        .await;
    let redirected = server.mock("GET", "/redirected.tgz").expect(0).create_async().await;

    let upstream = upstream(server.url(), auth_and_custom_headers());
    let result = upstream.fetch_revision_tarball_response("digest").await;

    assert!(matches!(
        result,
        Err(RegistryError::UpstreamStatus { status: 302, body, .. })
            if body == format!("{} (response body truncated)", "x".repeat(UPSTREAM_ERROR_BODY_LIMIT))
    ));
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[tokio::test]
async fn discovery_rejects_redirects_before_configured_headers_reach_the_target() {
    let mut target = mockito::Server::new_async().await;
    let redirected = target
        .mock("GET", "/-/v1/search")
        .match_header("authorization", mockito::Matcher::Missing)
        .expect(0)
        .create_async()
        .await;
    let mut source = mockito::Server::new_async().await;
    let redirect = source
        .mock("GET", "/-/v1/search")
        .match_query("text=foo")
        .match_header("authorization", "Bearer secret-token")
        .with_status(302)
        .with_header("location", &format!("{}/-/v1/search", target.url()))
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(source.url(), auth_and_custom_headers());
    let result = upstream.fetch_search("text=foo").await;

    assert!(
        matches!(result, Err(RegistryError::UpstreamStatus { status: 302, .. })),
        "expected the first redirect response, got {result:?}",
    );
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[test]
fn configured_headers_require_a_secure_same_origin_destination() {
    for base in ["http://registry.example", "https://registry.example", "http://127.0.0.1"] {
        let upstream = upstream(base.to_string(), auth_and_custom_headers());
        assert_eq!(
            upstream.request_headers(&format!("{base}/metadata")).is_empty(),
            base == "http://registry.example",
        );
        assert!(upstream.request_headers("https://other.example/metadata").is_empty());
    }
}

#[tokio::test]
async fn fetch_packument_sends_no_authorization_when_headers_empty() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::Modified(_)));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_modified_carries_the_body() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({ "name": "foo" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    let PackumentFetch::Modified(fetched) = outcome else { panic!("expected a body") };
    assert!(!fetched.bytes.is_empty(), "a modified fetch carries the packument body");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_replays_validators_and_handles_304() {
    let mut server = mockito::Server::new_async().await;
    // The mock only matches when both conditional headers are present,
    // so a `NotModified` outcome proves they rode along on the request.
    let mock = server
        .mock("GET", "/foo")
        .match_header("if-none-match", r#""abc123""#)
        .match_header("if-modified-since", "Wed, 21 Oct 2015 07:28:00 GMT")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let validators = CacheValidators {
        etag: Some(r#""abc123""#.to_string()),
        last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
    };
    let outcome = upstream.fetch_packument(&name, &validators).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::NotModified));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_304_without_validators_is_an_error() {
    let mut server = mockito::Server::new_async().await;
    // No conditional header is sent (empty validators), so a `304` here is
    // a misbehaving upstream — there's no body and nothing to revalidate
    // against. It must surface as an error, not a `NotModified` that the
    // caller could mistake for "keep serving the cache".
    let mock = server
        .mock("GET", "/foo")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .match_header("if-modified-since", mockito::Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let result = upstream.fetch_packument(&name, &CacheValidators::default()).await;

    assert!(result.is_err(), "an unconditional 304 must not be treated as NotModified");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_packument_maps_404_to_not_found() {
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/foo").with_status(404).expect(1).create_async().await;

    let upstream = upstream(server.url(), HeaderMap::new());
    let name = CanonicalPackageName::parse("foo", pnpr_package_name::Ecosystem::Npm).unwrap();
    let outcome = upstream.fetch_packument(&name, &CacheValidators::default()).await.unwrap();

    assert!(matches!(outcome, PackumentFetch::NotFound));
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_document_forwards_headers_and_accept_and_reports_the_final_url() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/simple/requests/")
        .match_header("authorization", "Bearer secret-token")
        .match_header("accept", "application/vnd.pypi.simple.v1+json")
        .with_body(r#"{"name":"requests"}"#)
        .expect(1)
        .create_async()
        .await;

    let upstream = upstream(format!("{}/simple/", server.url()), auth_and_custom_headers());
    let outcome = upstream
        .fetch_document("requests/", Some("application/vnd.pypi.simple.v1+json"), 1024)
        .await
        .unwrap();
    let FetchOutcome::Ok(document) = outcome else { panic!("expected a document") };
    assert_eq!(document.bytes, br#"{"name":"requests"}"#);
    assert_eq!(document.url, format!("{}/simple/requests/", server.url()));
    mock.assert_async().await;

    let missing = server.mock("GET", "/simple/nope/").with_status(404).create_async().await;
    let outcome = upstream.fetch_document("nope/", None, 1024).await.unwrap();
    assert!(matches!(outcome, FetchOutcome::NotFound));
    missing.assert_async().await;
}

#[tokio::test]
async fn fetch_document_rejects_a_body_over_the_limit() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("GET", "/se/rd/serde").with_body("x".repeat(64)).expect(2).create_async().await;
    let upstream = breaking_upstream(server.url(), 1);
    for _ in 0..2 {
        let err = upstream.fetch_document("se/rd/serde", None, 16).await.unwrap_err();
        assert!(matches!(err, RegistryError::UpstreamResponse { .. }), "{err:?}");
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn configured_headers_are_removed_on_metadata_redirects() {
    let mut source = mockito::Server::new_async().await;
    let mut target = mockito::Server::new_async().await;
    let target_mock = target
        .mock("GET", "/leak")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("x-org", mockito::Matcher::Missing)
        .with_body("metadata")
        .expect(1)
        .create_async()
        .await;
    let redirect = source
        .mock("GET", "/metadata")
        .with_status(302)
        .with_header("location", &format!("{}/leak", target.url()))
        .expect(1)
        .create_async()
        .await;
    let upstream = upstream(source.url(), auth_and_custom_headers());
    assert!(matches!(
        upstream.fetch_document("metadata", None, 1024).await.unwrap(),
        FetchOutcome::Ok(_)
    ));
    target_mock.assert_async().await;
    redirect.assert_async().await;
}
