use super::{
    AuthHeaders, FULL_META_DIR, FetchFullMetadataCachedOptions, PACKAGE_BODY, TempDir,
    ThrottledClient, corrupt_gzip_body_mock, fast_retry_opts, fetch_full_metadata_cached,
    get_pkg_mirror_path, load_meta_headers,
};

#[tokio::test]
async fn body_read_failure_retries_and_writes_mirror() {
    let mut server = mockito::Server::new_async().await;
    let first = corrupt_gzip_body_mock(&mut server).await;
    let second = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"after-retry""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        filter_metadata: false,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: fast_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("body read retries");
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    second.assert_async().await;

    // The retry's body is what gets persisted to the mirror.
    let mirror_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"after-retry""#));

    // A follow-up conditional GET answered 304 proves the persisted body is
    // a usable mirror, not just freshened headers over a missing/stale body.
    let not_modified = server.mock("GET", "/acme").with_status(304).expect(1).create_async().await;
    let cached_pkg =
        fetch_full_metadata_cached("acme", &opts).await.expect("mirror body readable after retry");
    assert_eq!(cached_pkg.name, "acme");
    not_modified.assert_async().await;
}
