use super::{PendingPrefetch, TarballDownload, run_tarball_download, without_store_hits};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
    store_index_key,
};
use pnpm_tarball::{MemCache, RetryOpts, TarballError};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tempfile::tempdir;

fn sample_index() -> PackageFilesIndex {
    let mut files = HashMap::new();
    files.insert(
        "package.json".to_string(),
        CafsFileInfo {
            checked_at: Some(1_700_000_000_000),
            digest: "abc".to_string(),
            mode: 0o644,
            size: 123,
        },
    );
    PackageFilesIndex {
        manifest: None,
        requires_build: Some(false),
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    }
}

fn pending(package_id: &str, integrity: &str) -> PendingPrefetch {
    PendingPrefetch {
        store_key: store_index_key(integrity, package_id),
        package_id: package_id.to_string(),
        package_url: format!("https://registry.example.com/{package_id}.tgz"),
        integrity: integrity.to_string(),
        revision_addressed: false,
    }
}

#[tokio::test]
async fn without_store_hits_drops_entries_with_an_index_row() {
    let store = tempdir().unwrap();
    let warm = pending("@foo/warm@1.0.0", "sha512-aGVsbG8=");
    let cold = pending("@foo/cold@1.0.0", "sha512-d29ybGQ=");
    {
        let idx = StoreIndex::open(store.path()).unwrap();
        idx.set(&warm.store_key, &sample_index()).unwrap();
    }
    let index = StoreIndex::open_readonly(store.path())
        .map(|idx| std::sync::Arc::new(std::sync::Mutex::new(idx)))
        .ok();
    assert!(index.is_some(), "readonly index should open after a write");

    let remaining = without_store_hits(index, vec![warm, cold]).await;

    let remaining_ids: Vec<&str> =
        remaining.iter().map(|entry| entry.package_id.as_str()).collect();
    assert_eq!(remaining_ids, ["@foo/cold@1.0.0"]);
}

#[tokio::test]
async fn without_store_hits_keeps_everything_when_no_index_is_readable() {
    let warm = pending("@foo/warm@1.0.0", "sha512-aGVsbG8=");
    let cold = pending("@foo/cold@1.0.0", "sha512-d29ybGQ=");

    let remaining = without_store_hits(None, vec![warm, cold]).await;

    assert_eq!(remaining.len(), 2);
}

fn revision_download(
    store_dir: &'static StoreDir,
    package_url: String,
    integrity: ssri::Integrity,
) -> TarballDownload {
    TarballDownload {
        http_client: Arc::new(ThrottledClient::default()),
        mem_cache: Arc::new(MemCache::new()),
        store_dir,
        store_index: None,
        store_index_writer: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        auth_headers: Arc::new(AuthHeaders::default()),
        retry_opts: RetryOpts {
            retries: 2,
            factor: 1,
            min_timeout: Duration::ZERO,
            max_timeout: Duration::ZERO,
        },
        requester: Arc::from(""),
        offline: false,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        package_id: "revision-pkg@1.0.0".to_string(),
        package_url,
        integrity,
        package_unpacked_size: None,
        package_file_count: None,
        revision_addressed: true,
    }
}

#[tokio::test]
async fn revision_prefetch_does_not_follow_redirects() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/revision.tgz")
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .expect(1)
        .create_async()
        .await;
    let redirected = server.mock("GET", "/redirected.tgz").expect(0).create_async().await;
    let store = tempdir().unwrap();
    let store_dir = Box::leak(Box::new(StoreDir::new(store.path())));
    let integrity = format!("sha512-{}==", "A".repeat(86)).parse().unwrap();

    let err = run_tarball_download(revision_download(
        store_dir,
        format!("{}/revision.tgz", server.url()),
        integrity,
    ))
    .await
    .expect_err("revision prefetch must reject the redirect");

    assert!(matches!(err, TarballError::HttpStatus(_)), "got {err:?}");
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[tokio::test]
async fn revision_prefetch_does_not_retry_a_transient_failure() {
    let mut server = mockito::Server::new_async().await;
    let failure =
        server.mock("GET", "/revision.tgz").with_status(503).expect(1).create_async().await;
    let store = tempdir().unwrap();
    let store_dir = Box::leak(Box::new(StoreDir::new(store.path())));
    let integrity = format!("sha512-{}==", "A".repeat(86)).parse().unwrap();

    let err = run_tarball_download(revision_download(
        store_dir,
        format!("{}/revision.tgz", server.url()),
        integrity,
    ))
    .await
    .expect_err("revision prefetch must not retry the transient failure");

    assert!(matches!(err, TarballError::HttpStatus(_)), "got {err:?}");
    failure.assert_async().await;
}
