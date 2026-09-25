use super::{super::CreateVirtualStore, DUMMY_SHA512, key, metadata_with_integrity};
use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{LockfileEntries, SnapshotEntry};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{StoreIndexWriter, store_index_key};
use pnpm_tarball::{SharedReportedProgressKeys, pending_progress_key};
use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::atomic::AtomicU8,
};

/// An optional package the dedupe observer found in the store at resolve
/// time, but whose fetch then fails, must not be reported as reused: the
/// swallowed failure claims its key so the observer's deferred report
/// skips it.
#[tokio::test]
async fn failed_optional_fetch_claims_a_pending_store_status() {
    let package_key = store_index_key(DUMMY_SHA512, "optional-dep@1.0.0");
    let progress_reported = SharedReportedProgressKeys::default();
    progress_reported.insert(pending_progress_key(&package_key));

    install_unfetchable_optional_dependency(&progress_reported).await;

    assert!(
        progress_reported.contains(&package_key),
        "the swallowed failure must claim the pending key: {progress_reported:?}",
    );
}
/// Without the observer's pending marker, a swallowed optional failure
/// leaves the reported keys alone.
#[tokio::test]
async fn failed_optional_fetch_without_a_pending_marker_claims_nothing() {
    let progress_reported = SharedReportedProgressKeys::default();

    install_unfetchable_optional_dependency(&progress_reported).await;

    assert!(progress_reported.is_empty(), "unexpected keys: {progress_reported:?}");
}

async fn install_unfetchable_optional_dependency(progress_reported: &SharedReportedProgressKeys) {
    let root = tempfile::tempdir().expect("create temp dir");
    let workspace_root = root.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");
    let modules_dir = workspace_root.join("node_modules");

    let mut config = Config::new();
    config.registry = "https://registry.test".to_string();
    config.store_dir = root.path().join("store").into();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    config.modules_dir = modules_dir;
    // The tarball is in neither the store nor a memory cache, so the
    // offline fetch fails on the fetch side, which an optional snapshot
    // swallows.
    config.offline = true;
    let config = config.leak();

    let package_key = key("optional-dep", "1.0.0");
    let snapshots = HashMap::from([(
        package_key.clone(),
        SnapshotEntry { optional: true, ..Default::default() },
    )]);
    let packages =
        HashMap::from([(package_key.without_peer(), metadata_with_integrity(DUMMY_SHA512))]);

    let allow_build_policy = AllowBuildPolicy::default();
    let layout = VirtualStoreLayout::new(
        config,
        None,
        Some(&snapshots),
        Some(&packages),
        Some(&allow_build_policy),
        None,
    );
    let skipped = SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let (store_index_writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);
    let requester = workspace_root.to_string_lossy().into_owned();

    let output = CreateVirtualStore {
        fetching: crate::VirtualStoreFetchInputs {
            http_client: &pnpm_network::ThrottledClient::default(),
            store_index_writer: &store_index_writer,
            store_context: None,
            cas_prefetch: None,
            progress_reported,
            tarball_mem_cache: None,
            custom_fetcher_session: None,
            planned_canonical_fetches: None,
        },
        selection: crate::SnapshotSelection {
            skipped: &skipped,
            include_optional: true,
            supported_architectures: None,
        },
        ctx: &crate::InstallContext {
            linker: crate::ModuleLinkerContext {
                layout: &layout,
                kind: NodeLinker::Isolated,
                bin_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            },
            config,
            workspace_root: &workspace_root,
            requester: &requester,

            allow_build_policy: &allow_build_policy,

            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
            dir_clone_cache: None,
        },

        entries: LockfileEntries { packages: Some(&packages), snapshots: Some(&snapshots) },
        current_entries: LockfileEntries::default(),

        dir_clone_cache: None,

        link_concurrency_probe: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("an optional fetch failure must not fail the install");

    drop(store_index_writer);
    writer_task.await.expect("join store-index writer").expect("flush store-index writer");

    assert_eq!(
        output.fetch_failed,
        HashSet::from([package_key]),
        "the unfetchable optional snapshot must be dropped",
    );
}
