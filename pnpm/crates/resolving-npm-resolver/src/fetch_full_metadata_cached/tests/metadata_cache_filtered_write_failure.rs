use super::{
    AuthHeaders, FULL_FILTERED_META_DIR, FetchFullMetadataCachedOptions, PACKAGE_BODY, TempDir,
    ThrottledClient, fetch_full_metadata_cached, get_pkg_mirror_path, no_retry_opts,
};

/// A filtered full document is stored with the NDJSON writer. A cache
/// directory that cannot be created must not fail the fetch.
#[cfg(unix)]
#[tokio::test]
async fn read_only_cache_dir_does_not_fail_filtered_metadata() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let mode = cache
        .path()
        .metadata()
        .expect("stat")
        .permissions()
        .mode();
    fs::set_permissions(cache.path(), fs::Permissions::from_mode(0o555)).expect("set read-only");

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        filter_metadata: true,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        http: crate::MetadataHttpClient {
            http_client: &http_client,
            auth_headers: &auth_headers,
            retry_opts: no_retry_opts(),
        },
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await
        .expect("read-only filtered metadata must not fail");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), FULL_FILTERED_META_DIR, &registry, "acme")
        .expect("filtered path");
    assert!(!mirror_path.exists(), "failed filtered write must not leave a mirror");

    let _ = fs::set_permissions(cache.path(), fs::Permissions::from_mode(mode));
}
