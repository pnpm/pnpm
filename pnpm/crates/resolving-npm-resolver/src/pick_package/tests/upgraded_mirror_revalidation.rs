use super::{
    ABBREVIATED_BODY, ABBREVIATED_META_DIR, AuthHeaders, InMemoryPackageMetaCache, PACKAGE_BODY,
    PARTIAL_TIME_PACKAGE_BODY, PickPackageContext, RetryOpts, TempDir, ThrottledClient, assert_eq,
    default_opts, get_pkg_mirror_path, load_meta, parse_cutoff, pick_package, range_spec,
    shared_packument_fetch_locker,
};
use crate::mirror::load_meta_headers;

/// The upgraded full document lands in the abbreviated mirror, so its entity
/// tag only validates the full representation. A later install must
/// revalidate that mirror as the full document with that tag, which a
/// registry with per-representation tags answers with `304`. Sending the
/// abbreviated `Accept` with only `If-Modified-Since` downloads a body from
/// registries that ignore that header, such as npmjs.org.
#[tokio::test]
async fn published_by_upgraded_mirror_revalidates_with_the_full_etag() {
    let mut server = mockito::Server::new_async().await;
    let (abbrev_mock, full_mock) = mock_upgrade(&mut server).await;
    let revalidation_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""full-etag""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let abbrev_path = upgrade_then_revalidate(&server, &cache_dir).await;

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
    revalidation_mock.assert_async().await;
    let persisted = load_meta(&abbrev_path).expect("abbreviated mirror readable");
    assert!(persisted.time.is_some(), "the 304 keeps the upgraded document in the mirror");
}

/// A changed full document answering that revalidation replaces the mirror
/// and keeps its new tag as the full document's, never as the abbreviated
/// mirror's own.
#[tokio::test]
async fn published_by_upgraded_mirror_records_a_changed_full_document_as_full() {
    let mut server = mockito::Server::new_async().await;
    let (abbrev_mock, full_mock) = mock_upgrade(&mut server).await;
    let revalidation_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""full-etag""#)
        .with_status(200)
        .with_header("etag", r#""changed-full-etag""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    let abbrev_path = upgrade_then_revalidate(&server, &cache_dir).await;

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
    revalidation_mock.assert_async().await;
    let headers = load_meta_headers(&abbrev_path).expect("headers readable");
    assert_eq!(headers.etag, None);
    assert_eq!(headers.full_etag.as_deref(), Some(r#""changed-full-etag""#));
}

/// A registry whose full document lacks a complete `time` map leaves the
/// upgraded mirror without one. Each later install revalidates the mirror's
/// full document with its tag, which is the request an upgrade would send,
/// so installs two and three send one conditional request each and download
/// no body.
#[tokio::test]
async fn published_by_upgrade_of_an_upgraded_mirror_revalidates_with_the_full_etag() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_header("etag", r#""abbreviated-etag""#)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("etag", r#""full-etag""#)
        .with_body(PARTIAL_TIME_PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    // Each later install's revalidation.
    let revalidation_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""full-etag""#)
        .with_status(304)
        .expect(2)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    install_ignoring_missing_time(
        &server,
        &cache_dir,
        &["upgrading install", "second install", "third install"],
    )
    .await;

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
    revalidation_mock.assert_async().await;
}

/// A changed full document answering that revalidation is the document an
/// upgrade would fetch, so the install sends no upgrade request after it,
/// even though the document still lacks a complete `time` map.
#[tokio::test]
async fn published_by_changed_full_document_without_complete_time_is_not_upgraded_again() {
    let mut server = mockito::Server::new_async().await;
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("etag", r#""full-etag""#)
        .with_body(PARTIAL_TIME_PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let revalidation_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", r#""full-etag""#)
        .with_status(200)
        .with_header("etag", r#""changed-full-etag""#)
        .with_body(PARTIAL_TIME_PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    let upgrade_mock = server
        .mock("GET", "/acme")
        .match_header("if-none-match", r#""changed-full-etag""#)
        .with_status(304)
        .expect(0)
        .create_async()
        .await;

    let cache_dir = TempDir::new().expect("tempdir");
    install_ignoring_missing_time(&server, &cache_dir, &["upgrading install", "second install"])
        .await;

    abbrev_mock.assert_async().await;
    full_mock.assert_async().await;
    revalidation_mock.assert_async().await;
    upgrade_mock.assert_async().await;
}

/// Run `installs` in order against `server`, each with a fresh in-memory
/// cache and the shared mirror in `cache_dir`, backdating the abbreviated
/// mirror after each so the next install revalidates it.
async fn install_ignoring_missing_time(
    server: &mockito::Server,
    cache_dir: &TempDir,
    installs: &[&str],
) {
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let mut opts = default_opts(&registry);
    opts.policy.published_by = Some(parse_cutoff("2023-01-01T00:00:00Z"));
    let abbrev_path =
        get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
            .expect("path");

    for &install in installs {
        let meta_cache = InMemoryPackageMetaCache::default();
        let fetch_locker = shared_packument_fetch_locker();
        let ctx = PickPackageContext {
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            cache_policy: crate::MetadataCachePolicy {
                network: pnpm_config::NetworkPolicy::Online,
                ignore_missing_time_field: true,
            },
            store_view: None,
            metadata: crate::MetadataRequestContext {
                meta_cache: &meta_cache,
                fetch_locker: &fetch_locker,
                cache_dir: Some(cache_dir.path()),
                http: crate::MetadataHttpClient {
                    http_client: &http_client,
                    auth_headers: &auth_headers,
                    retry_opts: RetryOpts::default(),
                },
            },
        };
        pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect(install);
        std::fs::File::options()
            .write(true)
            .open(&abbrev_path)
            .and_then(|file| file.set_modified(std::time::UNIX_EPOCH))
            .expect("backdate the mirror");
    }
}

/// One abbreviated `200` and one unconditional full `200`: the requests of the
/// install that upgrades `acme`.
async fn mock_upgrade(server: &mut mockito::Server) -> (mockito::Mock, mockito::Mock) {
    let abbrev_mock = server
        .mock("GET", "/acme")
        .match_header(
            "accept",
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        )
        .with_status(200)
        .with_header("etag", r#""abbreviated-etag""#)
        .with_body(ABBREVIATED_BODY)
        .expect(1)
        .create_async()
        .await;
    let full_mock = server
        .mock("GET", "/acme")
        .match_header("accept", "application/json; q=1.0, */*")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("etag", r#""full-etag""#)
        .with_body(PACKAGE_BODY)
        .expect(1)
        .create_async()
        .await;
    (abbrev_mock, full_mock)
}

/// Run an install that upgrades `acme` to full metadata, then a later one
/// that revalidates the mirror, and return the abbreviated mirror's path.
async fn upgrade_then_revalidate(
    server: &mockito::Server,
    cache_dir: &TempDir,
) -> std::path::PathBuf {
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let mut opts = default_opts(&registry);
    opts.policy.published_by = Some(parse_cutoff("2023-01-01T00:00:00Z"));
    let abbrev_path =
        get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
            .expect("path");

    for install in ["upgrading install", "revalidating install"] {
        let meta_cache = InMemoryPackageMetaCache::default();
        let fetch_locker = shared_packument_fetch_locker();
        let ctx = PickPackageContext {
            full_metadata: false,
            needs_full_metadata_for: None,
            filter_metadata: false,
            cache_policy: crate::MetadataCachePolicy {
                network: pnpm_config::NetworkPolicy::Online,
                ignore_missing_time_field: false,
            },
            store_view: None,
            metadata: crate::MetadataRequestContext {
                meta_cache: &meta_cache,
                fetch_locker: &fetch_locker,
                cache_dir: Some(cache_dir.path()),
                http: crate::MetadataHttpClient {
                    http_client: &http_client,
                    auth_headers: &auth_headers,
                    retry_opts: RetryOpts::default(),
                },
            },
        };
        let picked = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts)
            .await
            .expect(install);
        assert!(picked.meta.time.is_some(), "{install} should pick from the full document");
        // Date the mirror before the cutoff so the next install revalidates
        // it instead of trusting it as freshly written.
        std::fs::File::options()
            .write(true)
            .open(&abbrev_path)
            .and_then(|file| file.set_modified(std::time::UNIX_EPOCH))
            .expect("backdate the mirror");
    }
    abbrev_path
}
