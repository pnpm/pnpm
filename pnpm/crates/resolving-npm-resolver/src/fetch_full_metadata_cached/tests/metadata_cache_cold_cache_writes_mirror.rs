use super::{
    ABBREVIATED_META_DIR, ACCEPT_ABBREVIATED, AuthHeaders, FULL_FILTERED_META_DIR, FULL_META_DIR,
    FetchFullMetadataCachedOptions, FetchMetadataError, Matcher, PACKAGE_BODY, TempDir,
    ThrottledClient, assert_cache_loss_after_304_recovers, fast_retry_opts,
    fetch_full_metadata_cached, get_pkg_mirror_path, load_meta, load_meta_headers, no_retry_opts,
    remove_raced_mirror, write_stale_mirror,
};

#[tokio::test]
async fn cold_cache_writes_mirror_on_200() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
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
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");
    assert!(mirror_path.exists(), "mirror file written");
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"fresh""#));
}

#[tokio::test]
async fn offline_with_mirror_reads_cache_without_registry() {
    let mut server = mockito::Server::new_async().await;
    let no_network = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    write_stale_mirror(cache.path(), FULL_META_DIR, &registry);

    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: true,
        filter_metadata: false,
        offline: true,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("offline cache hit");
    assert_eq!(pkg.name, "acme");
    no_network.assert_async().await;
}

#[tokio::test]
async fn offline_without_mirror_errors_without_registry() {
    let mut server = mockito::Server::new_async().await;
    let no_network = server.mock("GET", "/acme").with_status(500).expect(0).create_async().await;

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
        offline: true,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("offline miss");
    assert!(matches!(
        error,
        FetchMetadataError::NoOfflineMeta { ref pkg_name, .. } if pkg_name == "acme"
    ));
    no_network.assert_async().await;
}

#[tokio::test]
async fn unsolicited_304_retries_without_cache() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
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
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("retry returns metadata");
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn repeated_unsolicited_304_reports_missing_cache() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .match_header("cache-control", Matcher::Missing)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("cache-control", "no-cache")
        .with_status(304)
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
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("304 needs a cache");
    dbg!(&error);
    assert!(matches!(
        error,
        FetchMetadataError::NotModifiedWithoutCache { ref pkg_name } if pkg_name == "acme"
    ));
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn full_metadata_cache_loss_after_304_retries_once_without_validators() {
    assert_cache_loss_after_304_recovers(true, FULL_META_DIR, true).await;
}

#[tokio::test]
async fn abbreviated_metadata_cache_loss_after_304_retries_once_without_validators() {
    assert_cache_loss_after_304_recovers(false, ABBREVIATED_META_DIR, false).await;
}

#[tokio::test]
async fn cache_loss_after_304_stops_after_one_fallback() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

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
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("fallback 304 fails");
    assert!(matches!(
        error,
        FetchMetadataError::NotModifiedWithoutCache { ref pkg_name } if pkg_name == "acme"
    ));
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn cache_loss_after_304_body_retry_remains_bypassed() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let broken = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
        .with_header("content-encoding", "gzip")
        .with_body("this is not valid gzip")
        .expect(1)
        .create_async()
        .await;
    let recovered = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(200)
        .with_header("etag", r#"W/"after-retry""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

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

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("body retry succeeds");
    assert_eq!(pkg.name, "acme");
    first.assert_async().await;
    broken.assert_async().await;
    recovered.assert_async().await;
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    assert_eq!(persisted.name, "acme");
    let headers = load_meta_headers(&mirror_path).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(r#"W/"after-retry""#));
}

#[tokio::test]
async fn cache_loss_after_304_registry_error_propagates() {
    let mut server = mockito::Server::new_async().await;
    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let mirror_path = write_stale_mirror(cache.path(), FULL_META_DIR, &registry);
    let raced_mirror = mirror_path.clone();
    let first = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"stale""#)
        .with_status(304)
        .with_body_from_request(move |_| {
            remove_raced_mirror(&raced_mirror);
            Vec::new()
        })
        .expect(1)
        .create_async()
        .await;
    let forbidden = server
        .mock("GET", "/acme")
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .match_header("cache-control", "no-cache")
        .with_status(403)
        .expect(1)
        .create_async()
        .await;

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
        retry_opts: no_retry_opts(),
    };

    let error = fetch_full_metadata_cached("acme", &opts).await.expect_err("403 propagates");
    assert!(matches!(
        error,
        FetchMetadataError::Network { ref error, .. }
            if error.status() == Some(reqwest::StatusCode::FORBIDDEN)
    ));
    first.assert_async().await;
    forbidden.assert_async().await;
}

#[test]
fn raced_mirror_removal_is_idempotent() {
    let cache = TempDir::new().expect("tempdir");
    let mirror_path = cache.path().join("mirror.json");
    std::fs::write(&mirror_path, "").expect("write mirror");

    remove_raced_mirror(&mirror_path);
    remove_raced_mirror(&mirror_path);
}

#[tokio::test]
async fn filtered_full_cache_writes_filtered_mirror_on_200() {
    let mut server = mockito::Server::new_async().await;
    let filtered_body = PACKAGE_BODY.replace(
        r#""dist": {"#,
        r#""readme": "drop me", "scripts": { "preinstall": "node build.js" }, "dist": {"#,
    );
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(filtered_body)
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
        filter_metadata: true,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 -> ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), FULL_FILTERED_META_DIR, &registry, "acme")
        .expect("filtered path");
    assert!(mirror_path.exists(), "filtered mirror file written");
    let unfiltered_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("full path");
    assert!(!unfiltered_path.exists(), "unfiltered mirror must not be written");
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let manifest = persisted.versions.get("1.0.0").expect("manifest");
    assert!(!manifest.other.contains_key("readme"));
    assert!(!manifest.other.contains_key("scripts"));
}

#[tokio::test]
async fn a_doc_served_with_the_abbreviated_content_type_is_cached_verbatim() {
    let mut server = mockito::Server::new_async().await;
    // A custom per-version field proves the fragment is mirrored verbatim
    // (no stripping) on the honored-header happy path.
    let abbreviated_body =
        PACKAGE_BODY.replace(r#""dist": {"#, r#""_cacheUntouchedMarker": "kept", "dist": {"#);
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        // Uppercase + a parameter: media-type detection must be
        // case-insensitive and drop parameters.
        .with_header("content-type", "APPLICATION/VND.NPM.INSTALL-V1+JSON; charset=utf-8")
        .with_body(abbreviated_body)
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
        full_metadata: false,
        filter_metadata: false,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("abbreviated path");
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let manifest = persisted.versions.get("1.0.0").expect("manifest");
    assert!(manifest.other.contains_key("_cacheUntouchedMarker"));
}

#[tokio::test]
async fn warm_cache_serves_from_mirror_on_304() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"v1""#)
        .with_status(304)
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
        retry_opts: no_retry_opts(),
    };

    let _first_pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 populates cache");
    first.assert_async().await;

    let second_pkg =
        fetch_full_metadata_cached("acme", &opts).await.expect("304 reads from mirror");
    second.assert_async().await;
    assert_eq!(second_pkg.name, "acme");
    assert_eq!(second_pkg.published_at("1.0.0"), Some("2025-01-10T08:30:00.000Z"));
}

#[tokio::test]
async fn a_304_renews_the_mirror_mtime() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"v1""#)
        .with_status(304)
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
        retry_opts: no_retry_opts(),
    };

    fetch_full_metadata_cached("acme", &opts).await.expect("200 populates cache");
    let mirror_path =
        get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");

    // Age the mirror far past any maturity cutoff.
    let aged = std::time::SystemTime::now() - std::time::Duration::from_hours(365 * 24);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&mirror_path)
        .expect("open mirror")
        .set_modified(aged)
        .expect("age mirror");

    fetch_full_metadata_cached("acme", &opts).await.expect("304 reads from mirror");

    let renewed = std::fs::metadata(&mirror_path).expect("stat mirror").modified().expect("mtime");
    let age = std::time::SystemTime::now().duration_since(renewed).expect("mtime in the past");
    assert!(
        age < std::time::Duration::from_mins(1),
        "mirror mtime must be renewed by the 304; still {age:?} old",
    );
}

#[tokio::test]
async fn stale_cache_refreshes_mirror_on_200() {
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"v1""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let updated_body = PACKAGE_BODY.replace("2025-01-10T08:30:00.000Z", "2025-03-01T00:00:00.000Z");
    let second = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#"W/"v1""#)
        .with_status(200)
        .with_header("etag", r#"W/"v2""#)
        .with_body(updated_body)
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
        retry_opts: no_retry_opts(),
    };

    let _ = fetch_full_metadata_cached("acme", &opts).await.expect("populate");
    first.assert_async().await;

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("refresh");
    second.assert_async().await;
    assert_eq!(pkg.published_at("1.0.0"), Some("2025-03-01T00:00:00.000Z"));
    let mirror = get_pkg_mirror_path(cache.path(), FULL_META_DIR, &registry, "acme").expect("path");
    let reloaded = load_meta(&mirror).expect("mirror readable");
    assert_eq!(reloaded.etag.as_deref(), Some(r#"W/"v2""#));
}

#[tokio::test]
async fn no_cache_dir_skips_mirror_io() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .with_status(200)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: None,
        full_metadata: true,
        filter_metadata: false,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_cache_dir_does_not_fail_the_call() {
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
    let mode = cache.path().metadata().expect("stat").permissions().mode();
    fs::set_permissions(cache.path(), fs::Permissions::from_mode(0o555)).expect("set read-only");

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
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("read-only must not fail");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    // Restore so TempDir's drop can clean up.
    let _ = fs::set_permissions(cache.path(), fs::Permissions::from_mode(mode));
}
