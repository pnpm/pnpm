use super::{
    AuthHeaders, FULL_META_DIR, FetchFullMetadataCachedOptions, FetchMetadataError, Matcher,
    PACKAGE_BODY, TempDir, ThrottledClient, fetch_full_metadata_cached,
    fetch_full_metadata_projected, load_meta, no_retry_opts, write_stale_mirror,
};

fn count_versions(meta: &pnpm_registry::Package) -> usize {
    meta.versions.iter_policy_fields().count()
}

/// Breaks the bytes of the `1.0.0` fragment without moving any span, so only
/// that fragment fails to parse.
fn damage_fragment(mirror_path: &std::path::Path) {
    let mut bytes = std::fs::read(mirror_path).expect("read mirror");
    let marker = b"sha512-AAAA";
    let at = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("1.0.0 fragment in the mirror");
    bytes[at + 3] = b'"';
    std::fs::write(mirror_path, &bytes).expect("write damaged mirror");
}

fn options<'a>(
    registry: &'a str,
    cache_dir: &'a std::path::Path,
    offline: bool,
    http_client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
) -> FetchFullMetadataCachedOptions<'a> {
    FetchFullMetadataCachedOptions {
        registry,
        cache_dir: Some(cache_dir),
        full_metadata: true,
        filter_metadata: false,
        offline,
        priority: pnpm_network::UNPRIORITIZED,
        http: crate::MetadataHttpClient { http_client, auth_headers, retry_opts: no_retry_opts() },
    }
}

#[tokio::test]
async fn offline_over_a_damaged_fragment_is_no_offline_meta() {
    let server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    damage_fragment(&mirror_path);

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = options(&registry, cache.path(), true, &http_client, &auth_headers);
    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("damaged mirror");
    assert!(matches!(error, FetchMetadataError::NoOfflineMeta { .. }), "got {error:?}");
}

#[tokio::test]
async fn a_304_over_a_damaged_fragment_refetches_and_rewrites_the_mirror() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    damage_fragment(&mirror_path);
    let revalidate = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let refetch = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = options(&registry, cache.path(), false, &http_client, &auth_headers);
    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("refetched metadata");
    assert!(pkg.versions.get("1.0.0").is_some());
    assert!(!pkg.versions.has_corrupt_mirror_fragment());
    let persisted = load_meta(&mirror_path).expect("rewritten mirror");
    assert!(persisted.versions.get("1.0.0").is_some());
    revalidate.assert_async().await;
    refetch.assert_async().await;
}

#[tokio::test]
async fn a_projection_over_a_damaged_fragment_offline_is_no_offline_meta() {
    let server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    damage_fragment(&mirror_path);

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = options(&registry, cache.path(), true, &http_client, &auth_headers);
    let error = fetch_full_metadata_projected("acme", &opts, count_versions)
        .await
        .expect_err("damaged mirror");
    assert!(matches!(error, FetchMetadataError::NoOfflineMeta { .. }), "got {error:?}");
}

#[tokio::test]
async fn a_projection_over_a_damaged_fragment_projects_the_refetched_document() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    damage_fragment(&mirror_path);
    let revalidate = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let refetch = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = options(&registry, cache.path(), false, &http_client, &auth_headers);
    let versions = fetch_full_metadata_projected("acme", &opts, count_versions)
        .await
        .expect("refetched metadata");
    assert_eq!(versions, 1);
    revalidate.assert_async().await;
    refetch.assert_async().await;
}
