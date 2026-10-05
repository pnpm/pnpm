use super::{
    super::{
        CreateVirtualStore, CreateVirtualStoreOutput, SnapshotCacheKey, cas::retain_fetch_pass_rows,
    },
    DUMMY_SHA512, key, metadata_with_integrity, snapshot_with_dep, take_prefetch_starts,
};
use crate::{
    AllowBuildPolicy, CasPathsByPkgId, HoistedPackageFiles, SkippedSnapshots, VirtualStoreLayout,
};
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_lockfile::{LockfileEntries, PackageKey, PkgIdWithPatchHash, SnapshotEntry};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, StoreIndex, StoreIndexWriter, store_index_key,
};
use pnpm_tarball::{PrefetchResult, SharedReportedProgressKeys};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU8},
};

/// Seed a store-index row and CAS blobs for `package_key` under `config`'s store.
fn seed_store_row(config: &Config, package_key: &PackageKey) -> String {
    let manifest = format!(r#"{{"name":"{}","version":"1.0.0"}}"#, package_key.name);
    let mut files = HashMap::new();
    for (path, content) in
        [("package.json", manifest.as_bytes()), ("index.js", b"module.exports = true\n".as_slice())]
    {
        let (_, digest) = config.store_dir.write_cas_file(content, false).expect("write file");
        files.insert(
            path.to_string(),
            CafsFileInfo {
                digest: format!("{digest:x}"),
                mode: 0o644,
                size: content.len() as u64,
                checked_at: None,
            },
        );
    }
    let row_key = store_index_key(DUMMY_SHA512, &package_key.without_peer().pkg_id());
    StoreIndex::open_in(&config.store_dir)
        .expect("open store index")
        .set(
            &row_key,
            &PackageFilesIndex {
                manifest: None,
                requires_build: Some(false),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files,
                side_effects: None,
                remote_side_effects_quarantine: None,
            },
        )
        .expect("seed store index");
    row_key
}

#[tokio::test]
async fn loaded_materialization_reuses_the_fetch_pass_prefetch() {
    let root = tempfile::tempdir().expect("create temp dir");
    let workspace_root = root.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");
    let modules_dir = workspace_root.join("node_modules");

    let mut config = Config::new();
    config.registry = "https://registry.test".to_string();
    config.store_dir = root.path().join("store").into();
    config.modules_dir = modules_dir.clone();
    config.install_state_dir = modules_dir.join(".pacquet");
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = root.path().join("links");
    config.package_import_method = PackageImportMethod::Copy;
    config.offline = true;
    config.node_linker = NodeLinker::Loaded;
    config.node_linker_excluded = vec!["excluded".to_string()];
    let config = config.leak();

    let excluded = key("excluded", "1.0.0");
    let child = key("child", "1.0.0");
    let loaded = key("loaded", "1.0.0");
    let snapshots = HashMap::from([
        (excluded.clone(), snapshot_with_dep("child", "1.0.0")),
        (child.clone(), SnapshotEntry::default()),
        (loaded.clone(), SnapshotEntry::default()),
    ]);
    let packages: HashMap<_, _> = snapshots
        .keys()
        .map(|snapshot_key| (snapshot_key.without_peer(), metadata_with_integrity(DUMMY_SHA512)))
        .collect();
    let mut row_keys: Vec<String> = snapshots
        .keys()
        .map(|snapshot_key| seed_store_row(config, snapshot_key))
        .collect();
    row_keys.sort_unstable();

    let allow_build_policy = AllowBuildPolicy::default();
    let layout = VirtualStoreLayout::new(
        config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        Some(&allow_build_policy),
        None,
    );
    let skipped = SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let progress_reported = SharedReportedProgressKeys::default();
    let (store_index_writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);
    let requester = workspace_root.to_string_lossy().into_owned();
    take_prefetch_starts();

    let output = CreateVirtualStore {
        fetching: crate::VirtualStoreFetchInputs {
            http_client: &pnpm_network::ThrottledClient::default(),
            store_index_writer: &store_index_writer,
            store_context: None,
            cas_prefetch: None,
            progress_reported: &progress_reported,
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
            caches: crate::InstallCaches {
                logged_methods: &logged_methods,
                git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
                materialized_graph: Arc::default(),
                dir_clone_cache: None,
            },
            linker: crate::ModuleLinkerContext {
                layout: &layout,
                kind: NodeLinker::Loaded,
                bin_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            },
            config,
            workspace_root: &workspace_root,
            requester: &requester,
            allow_build_policy: &allow_build_policy,
        },
        entries: LockfileEntries { packages: Some(&packages), snapshots: Some(&snapshots) },
        current_entries: LockfileEntries::default(),
        importers: &HashMap::new(),
        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<SilentReporter>()
    .await
    .expect("loaded install succeeds");
    drop(store_index_writer);
    writer_task.await.expect("join store-index writer").expect("flush store-index writer");

    let mut starts = take_prefetch_starts();
    assert_eq!(starts.len(), 1, "only the fetch pass reads the store index: {starts:?}");
    starts[0].sort_unstable();
    assert_eq!(starts[0], row_keys);

    for selected in [&excluded, &child] {
        let index_js = layout
            .slot_dir(selected)
            .join("node_modules")
            .join(selected.name.to_string())
            .join("index.js");
        assert!(index_js.is_file(), "{selected} materialized from the shared rows");
    }
    assert!(!layout.slot_dir(&loaded).exists(), "the loaded package is not materialized");
    let cas_paths = output.cas_paths_by_pkg_id.expect("loaded installs keep the CAS paths");
    assert_eq!(cas_paths.len(), 3);
}

fn row_cas_paths(file: &str) -> Arc<HashMap<String, PathBuf>> {
    Arc::new(HashMap::from([("index.js".to_string(), PathBuf::from(file))]))
}

fn downloaded(
    cas_paths: &Arc<HashMap<String, PathBuf>>,
    source_is_mutable: bool,
) -> HoistedPackageFiles {
    HoistedPackageFiles { cas_paths: Arc::clone(cas_paths), source_is_mutable, source_exists: true }
}

#[test]
fn fetch_pass_rows_keep_the_selection_and_its_downloads() {
    let cache_key = |value: &str, is_git_hosted: bool| {
        Ok(SnapshotCacheKey { value: Some(value.to_string()), is_git_hosted })
    };
    let cache_keys = HashMap::from([
        (key("warm", "1.0.0"), cache_key("warm-row", false)),
        (key("downloaded", "1.0.0"), cache_key("downloaded-row", false)),
        (key("git", "1.0.0"), cache_key("git-row", true)),
        (key("mutable", "1.0.0"), cache_key("mutable-row", false)),
    ]);
    let mut rows = PrefetchResult::default();
    for row in ["warm-row", "unselected-row"] {
        rows.cas_paths.insert(row.to_string(), row_cas_paths(row));
        rows.requires_build.insert(row.to_string(), false);
    }
    let downloads = row_cas_paths("downloaded");
    let by_pkg_id: CasPathsByPkgId = [
        ("downloaded@1.0.0", downloaded(&downloads, false)),
        ("git@1.0.0", downloaded(&row_cas_paths("git"), false)),
        ("mutable@1.0.0", downloaded(&row_cas_paths("mutable"), true)),
    ]
    .into_iter()
    .map(|(pkg_id, files)| (PkgIdWithPatchHash::from(pkg_id.to_string()), files))
    .collect();
    let fetched = CreateVirtualStoreOutput {
        package_manifests: HashMap::new(),
        side_effects_maps_by_snapshot: HashMap::new(),
        requires_build_by_snapshot: HashMap::from([(key("downloaded", "1.0.0"), true)]),
        materialized_snapshots: Vec::new(),
        fetch_failed: HashSet::new(),
        cas_paths_by_pkg_id: Some(by_pkg_id),
    };

    retain_fetch_pass_rows(&mut rows, &cache_keys, &fetched);

    let mut kept: Vec<&str> = rows.cas_paths
        .keys()
        .map(String::as_str)
        .collect();
    kept.sort_unstable();
    assert_eq!(kept, ["downloaded-row", "warm-row"]);
    assert!(Arc::ptr_eq(&rows.cas_paths["downloaded-row"], &downloads));
    assert_eq!(rows.requires_build.get("downloaded-row"), Some(&true));
    assert!(!rows.requires_build.contains_key("unselected-row"));
}
