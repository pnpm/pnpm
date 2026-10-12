use super::{
    TarballDownload,
    lockfile_entries::{
        PendingPrefetch, registry_entry, without_store_hits, without_verified_store_hits,
    },
    run_tarball_download,
};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_reporter::SilentReporter;
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
        idx.set(&warm.store_key, &sample_index())
            .unwrap();
    }
    let index = StoreIndex::open_readonly(store.path())
        .map(|idx| std::sync::Arc::new(std::sync::Mutex::new(idx)))
        .ok();
    assert!(index.is_some(), "readonly index should open after a write");

    let remaining = without_store_hits(index, vec![warm, cold]).await;

    let remaining_ids: Vec<&str> = remaining
        .iter()
        .map(|entry| entry.package_id.as_str())
        .collect();
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
        mem_cache: Arc::new(MemCache::new()),
        requester: Arc::from(""),
        store: pnpm_tarball::ArchiveStoreContext {
            dir: store_dir,
            index: None,
            index_writer: None,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            verify_integrity: true,
            strict_pkg_content_check: true,
            prefetched_cas_paths: None,
        },
        progress_reported: None,
        fetching: crate::tarball_prefetch::PrefetchHttpClient {
            http_client: Arc::new(ThrottledClient::default()),
            auth_headers: Arc::new(AuthHeaders::default()),
            retry_opts: RetryOpts {
                retries: 2,
                factor: 1,
                min_timeout: Duration::ZERO,
                max_timeout: Duration::ZERO,
            },
            offline: false,
        },
        package: crate::tarball_prefetch::TarballDownloadPackage {
            id: "revision-pkg@1.0.0".to_string(),
            url: package_url,
            integrity,
            unpacked_size: None,
            file_count: None,
            revision_addressed: true,
        },
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
    let redirected = server
        .mock("GET", "/redirected.tgz")
        .expect(0)
        .create_async()
        .await;
    let store = tempdir().unwrap();
    let store_dir = Box::leak(Box::new(StoreDir::new(store.path())));
    let integrity = format!("sha512-{}==", "A".repeat(86)).parse().unwrap();

    let err = run_tarball_download::<SilentReporter>(revision_download(
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
    let failure = server
        .mock("GET", "/revision.tgz")
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let store = tempdir().unwrap();
    let store_dir = Box::leak(Box::new(StoreDir::new(store.path())));
    let integrity = format!("sha512-{}==", "A".repeat(86)).parse().unwrap();

    let err = run_tarball_download::<SilentReporter>(revision_download(
        store_dir,
        format!("{}/revision.tgz", server.url()),
        integrity,
    ))
    .await
    .expect_err("revision prefetch must not retry the transient failure");

    assert!(matches!(err, TarballError::HttpStatus(_)), "got {err:?}");
    failure.assert_async().await;
}

/// The store fetch stages what the install itself could fetch, so an
/// entry whose integrity has nothing to check is an error rather than a
/// package left out of the store.
#[test]
fn an_unfetchable_registry_entry_fails_the_staging() {
    let lockfile: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters: {}\npackages:\n  foo@1.0.0:\n    resolution: {integrity: ''}\nsnapshots:\n  foo@1.0.0: {}\n",
    )
    .expect("parse lockfile");
    let (key, metadata) = lockfile.packages
        .as_ref()
        .expect("packages")
        .iter()
        .next()
        .expect("entry");
    let config = pnpm_config::Config::new().leak();

    let staged = registry_entry(key, metadata, config, None);

    assert!(matches!(
        staged,
        Err(pnpm_deps_restorer::InstallPackageBySnapshotError::MissingTarballIntegrity { .. }),
    ));
}

/// The store-only fetch has no materialization after it to download a
/// package whose index row outlived its files, so the verified lookup
/// treats such a row as missing.
#[tokio::test]
async fn a_row_whose_files_are_gone_is_fetched_again() {
    let store = tempdir().unwrap();
    let warm = pending("@foo/warm@1.0.0", "sha512-aGVsbG8=");
    {
        let idx = StoreIndex::open(store.path()).unwrap();
        idx.set(&warm.store_key, &sample_index())
            .unwrap();
    }
    let store_dir: &'static StoreDir = Box::leak(Box::new(StoreDir::new(store.path())));
    let context = pnpm_tarball::ArchiveStoreContext {
        dir: store_dir,
        index: StoreIndex::open_readonly(store.path())
            .map(|idx| Arc::new(std::sync::Mutex::new(idx)))
            .ok(),
        index_writer: None,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        verify_integrity: true,
        strict_pkg_content_check: true,
        prefetched_cas_paths: None,
    };
    assert!(context.index.is_some(), "readonly index should open after a write");

    let unverified = without_store_hits(
        context.index.clone(),
        vec![pending("@foo/warm@1.0.0", "sha512-aGVsbG8=")],
    )
    .await;
    let missing = without_verified_store_hits(&context, true, vec![warm]).await;

    assert!(unverified.is_empty(), "the row alone satisfies the speculative prefetch");
    assert_eq!(missing.len(), 1, "the files behind the row are gone, so the fetch downloads it");
}
