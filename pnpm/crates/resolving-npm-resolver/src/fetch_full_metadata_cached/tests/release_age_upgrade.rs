use mockito::Matcher;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_registry::Package;
use tempfile::TempDir;

use super::{
    super::fetch_release_age_upgrade, ABBREVIATED_META_DIR, FetchFullMetadataCachedOptions,
    PACKAGE_BODY, get_pkg_mirror_path, load_meta, load_meta_headers, no_retry_opts,
    save_meta_indexed,
};
use crate::mirror::save_fail::fail_next_mirror_save;

const ACCEPT_FULL: &str = "application/json; q=1.0, */*";
const ABBREVIATED_ETAG: &str = r#""abbreviated-etag""#;
const FULL_ETAG: &str = r#""full-etag""#;
const MODIFIED: &str = "2025-01-15T12:00:00.000Z";

/// A temporary cache directory holding the abbreviated mirror of `acme`, as
/// the abbreviated fetch recorded it.
struct AbbreviatedMirror {
    cache: TempDir,
    registry: String,
    http_client: ThrottledClient,
    auth_headers: AuthHeaders,
}

impl AbbreviatedMirror {
    fn new(server: &mockito::ServerGuard) -> Self {
        let mirror = AbbreviatedMirror {
            cache: TempDir::new().expect("tempdir"),
            registry: format!("{}/", server.url()),
            http_client: ThrottledClient::default(),
            auth_headers: AuthHeaders::default(),
        };
        let meta: Package = serde_json::from_str(PACKAGE_BODY).expect("parse packument");
        save_meta_indexed(&mirror.path(), &meta, Some(ABBREVIATED_ETAG), false)
            .expect("seed abbreviated mirror");
        mirror
    }

    fn path(&self) -> std::path::PathBuf {
        get_pkg_mirror_path(self.cache.path(), ABBREVIATED_META_DIR, &self.registry, "acme")
            .expect("abbreviated mirror path")
    }

    fn opts(&self) -> FetchFullMetadataCachedOptions<'_> {
        FetchFullMetadataCachedOptions {
            registry: &self.registry,
            cache_dir: Some(self.cache.path()),
            full_metadata: false,
            filter_metadata: false,
            offline: false,
            priority: pnpm_network::UNPRIORITIZED,
            http: crate::MetadataHttpClient {
                http_client: &self.http_client,
                auth_headers: &self.auth_headers,
                retry_opts: no_retry_opts(),
            },
        }
    }
}

async fn full_document_mock(
    server: &mut mockito::ServerGuard,
    cache_control: Option<&str>,
) -> mockito::Mock {
    let mut mock = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_FULL)
        .match_header("if-none-match", Matcher::Missing)
        .match_header("if-modified-since", Matcher::Missing)
        .with_status(200)
        .with_header("etag", FULL_ETAG)
        .with_body(PACKAGE_BODY);
    if let Some(cache_control) = cache_control {
        mock = mock.with_header("cache-control", cache_control);
    }
    mock.expect(1).create_async().await
}

#[tokio::test]
async fn an_upgrade_over_an_abbreviated_mirror_sends_no_validators_and_records_the_full_etag() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let mock = full_document_mock(&mut server, None).await;

    let meta = fetch_release_age_upgrade("acme", &mirror.opts(), true).await.expect("upgrade");
    assert_eq!(meta.name, "acme");
    mock.assert_async().await;

    let headers = load_meta_headers(&mirror.path()).expect("headers readable");
    assert_eq!(headers.etag, None);
    assert_eq!(headers.full_etag.as_deref(), Some(FULL_ETAG));
    assert_eq!(headers.modified.as_deref(), Some(MODIFIED));
    assert!(!headers.uncacheable);
}

#[tokio::test]
async fn an_upgrade_records_an_uncacheable_full_response() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let mock = full_document_mock(&mut server, Some("no-store")).await;

    fetch_release_age_upgrade("acme", &mirror.opts(), true).await.expect("upgrade");
    mock.assert_async().await;

    let headers = load_meta_headers(&mirror.path()).expect("headers readable");
    assert!(headers.uncacheable);
    assert_eq!(headers.etag, None);
}

#[tokio::test]
async fn an_upgraded_mirror_is_revalidated_with_its_full_etag() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let upgrade = full_document_mock(&mut server, None).await;
    fetch_release_age_upgrade("acme", &mirror.opts(), true).await.expect("first upgrade");
    upgrade.assert_async().await;

    let revalidation = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_FULL)
        .match_header("if-none-match", FULL_ETAG)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;
    let meta =
        fetch_release_age_upgrade("acme", &mirror.opts(), true).await.expect("second upgrade");
    assert_eq!(meta.name, "acme");
    revalidation.assert_async().await;
}

#[tokio::test]
async fn a_dry_run_upgrade_leaves_the_mirror_untouched() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let backdated = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(mirror.path())
        .and_then(|file| file.set_modified(backdated))
        .expect("backdate the mirror");
    let before = std::fs::read(mirror.path()).expect("read mirror");
    let mock = full_document_mock(&mut server, None).await;

    let meta = fetch_release_age_upgrade("acme", &mirror.opts(), false).await.expect("upgrade");
    assert_eq!(meta.name, "acme");
    mock.assert_async().await;

    assert_eq!(std::fs::read(mirror.path()).expect("read mirror"), before);
    let mtime = std::fs::metadata(mirror.path()).and_then(|metadata| metadata.modified());
    assert_eq!(mtime.expect("mirror mtime"), backdated);
}

#[tokio::test]
async fn a_failed_save_of_an_uncacheable_upgrade_removes_the_previous_mirror() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let mock = full_document_mock(&mut server, Some("no-store")).await;

    fail_next_mirror_save(&mirror.path());
    fetch_release_age_upgrade("acme", &mirror.opts(), true).await.expect("upgrade");
    mock.assert_async().await;

    assert!(!mirror.path().exists());
}

#[tokio::test]
async fn a_failed_save_of_a_cacheable_upgrade_keeps_the_previous_mirror() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let mock = full_document_mock(&mut server, None).await;

    fail_next_mirror_save(&mirror.path());
    fetch_release_age_upgrade("acme", &mirror.opts(), true).await.expect("upgrade");
    mock.assert_async().await;

    let headers = load_meta_headers(&mirror.path()).expect("previous mirror remains");
    assert_eq!(headers.etag.as_deref(), Some(ABBREVIATED_ETAG));
    assert!(load_meta(&mirror.path()).is_some());
}

/// A `304` to the validator-free upgrade request is about nothing the mirror
/// holds. It must fail the upgrade, for the caller to decline, and leave the
/// abbreviated mirror unchanged rather than renew it as revalidated.
#[tokio::test]
async fn an_unsolicited_304_to_an_upgrade_leaves_the_abbreviated_mirror_alone() {
    let mut server = mockito::Server::new_async().await;
    let mirror = AbbreviatedMirror::new(&server);
    let backdated = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(mirror.path())
        .and_then(|file| file.set_modified(backdated))
        .expect("backdate the mirror");
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_FULL)
        .match_header("if-none-match", Matcher::Missing)
        .with_status(304)
        .expect(2)
        .create_async()
        .await;

    let error = fetch_release_age_upgrade("acme", &mirror.opts(), true).await
        .expect_err("an unsolicited 304 cannot upgrade");
    assert!(
        matches!(error, crate::FetchMetadataError::NotModifiedWithoutCache { .. }),
        "expected NotModifiedWithoutCache, got {error:?}",
    );
    mock.assert_async().await;

    let mtime = std::fs::metadata(mirror.path()).and_then(|metadata| metadata.modified());
    assert_eq!(mtime.expect("mirror mtime"), backdated);
    let headers = load_meta_headers(&mirror.path()).expect("headers readable");
    assert_eq!(headers.etag.as_deref(), Some(ABBREVIATED_ETAG));
}
