use super::{super::host_platform_selector, leaked_offline_config, registry_metadata};
use pnpm_lockfile::PackageKey;
use pretty_assertions::assert_eq;

/// On the fresh-resolve path the resolve-time prefetcher may already
/// have a package's tarball download finished (or in flight) in the
/// shared mem cache by the time the cold batch reaches it. The cold
/// batch must reuse that download via the mem cache rather than racing
/// a second fetch of the same bytes.
///
/// Seed the mem cache with a finished download keyed by the exact URL
/// the registry resolution derives, then run the cold-batch installer
/// with `tarball_mem_cache: Some(..)`. It must return the seeded CAS
/// map without touching the network — proven here by `offline: true`,
/// which makes any fall-through to the download path error out.
#[tokio::test]
async fn cold_batch_reuses_in_flight_prefetch_from_mem_cache() {
    use pnpm_tarball::{CacheValue, MemCache};
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, atomic::AtomicU8},
    };

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());

    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse key");
    // Mirror `tarball_url_and_integrity`'s registry-URL derivation so
    // the seeded mem-cache key matches what the installer looks up.
    let tarball_url = "https://registry.test/foo/-/foo-1.0.0.tgz".to_string();

    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        tarball_url,
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded.clone())))),
    );

    let layout = crate::VirtualStoreLayout::legacy(store_tmp.path().join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let verified_files_cache = pnpm_store_dir::SharedVerifiedFilesCache::default();
    let metadata = registry_metadata();
    let snapshot = pnpm_lockfile::SnapshotEntry::default();

    let cas_paths = super::super::InstallPackageBySnapshot {
        ctx: &crate::InstallContext {
            config,
            workspace_root: store_tmp.path(),
            requester: "/project",
            layout: &layout,
            node_linker: pnpm_config::NodeLinker::Hoisted,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        store_index: None,
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: Some(&mem_cache),
        verified_files_cache: &verified_files_cache,
        skipped: &skipped,
        include_optional_dependencies: true,
        runtime_platform_selector: &host_platform_selector(),
        // Hoisted skips slot materialization, so the test exercises
        // only the download-coordination branch and gets the CAS map
        // back directly.
        custom_fetcher_session: None,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>(&package_key, &metadata, &snapshot)
    .await
    .expect("cold batch must reuse the prefetched download instead of fetching");

    assert_eq!(cas_paths.cas_paths, seeded);

    drop(store_tmp);
}
/// `tarball_mem_cache: None` is the no-prefetcher case (e.g. a plain
/// `--frozen-lockfile` install without pnpr). That path must go straight
/// to the download (here blocked by `offline: true`), never consulting a
/// mem cache — the contrast that proves the coordination above is gated
/// on `Some(..)`, not unconditional. A populated cache is supplied and
/// must be ignored.
#[tokio::test]
async fn without_mem_cache_skips_coordination_and_downloads() {
    use crate::InstallPackageBySnapshotError;
    use pnpm_tarball::{CacheValue, MemCache, TarballError};
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, atomic::AtomicU8},
    };

    let store_tmp = tempfile::tempdir().expect("tempdir");
    let config = leaked_offline_config("https://registry.test", store_tmp.path());

    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse key");

    // A populated mem cache that the `None` path must ignore.
    let seeded: HashMap<String, PathBuf> =
        HashMap::from([("package.json".to_string(), store_tmp.path().join("blob"))]);
    let mem_cache = Arc::new(MemCache::default());
    mem_cache.insert(
        "https://registry.test/foo/-/foo-1.0.0.tgz".to_string(),
        Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(seeded)))),
    );

    let layout = crate::VirtualStoreLayout::legacy(store_tmp.path().join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let verified_files_cache = pnpm_store_dir::SharedVerifiedFilesCache::default();
    let metadata = registry_metadata();
    let snapshot = pnpm_lockfile::SnapshotEntry::default();

    let err = super::super::InstallPackageBySnapshot {
        ctx: &crate::InstallContext {
            config,
            workspace_root: store_tmp.path(),
            requester: "/project",
            layout: &layout,
            node_linker: pnpm_config::NodeLinker::Hoisted,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        store_index: None,
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache: None,
        verified_files_cache: &verified_files_cache,
        skipped: &skipped,
        include_optional_dependencies: true,
        runtime_platform_selector: &host_platform_selector(),
        custom_fetcher_session: None,
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>(&package_key, &metadata, &snapshot)
    .await
    .expect_err("None path must skip the mem cache and hit the offline-gated download");

    assert!(
        matches!(
            err,
            InstallPackageBySnapshotError::DownloadTarball(TarballError::NoOfflineTarball { .. }),
        ),
        "expected the offline download gate, got {err:?}",
    );

    drop(store_tmp);
}
