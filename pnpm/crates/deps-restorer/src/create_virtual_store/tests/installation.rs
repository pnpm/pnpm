use super::{
    super::{
        CreateVirtualStore, CreateVirtualStoreStoreContext, removed_child_aliases,
        snapshot_deps_equal,
    },
    DUMMY_SHA512, SeededStoreInstall, key, metadata_with_integrity, name, snapshot,
    snapshot_with_dep,
};
use pnpm_lockfile::{LockfileEntries, SnapshotDepRef, SnapshotEntry};
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, fs, sync::atomic::AtomicU8};

#[test]
fn removed_child_aliases_excludes_self_and_unchanged_sets() {
    let self_name = name("host");
    // The slot lists itself as a dependency and is otherwise unchanged.
    let current = snapshot(&["host", "kept"], &[]);
    let wanted = snapshot(&["kept"], &[]);

    let removed = removed_child_aliases(&current, &wanted, &self_name);

    assert!(removed.is_empty(), "self and still-present children must not be removed: {removed:?}");
}
#[tokio::test]
async fn shared_store_context_materializes_a_warm_package() {
    use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
    use pnpm_config::{Config, NodeLinker, PackageImportMethod};
    use pnpm_store_dir::{
        CafsFileInfo, PackageFilesIndex, SharedVerifiedFilesCache, StoreDir, StoreIndex,
        StoreIndexWriter, store_index_key,
    };
    use pnpm_tarball::SharedReportedProgressKeys;

    let root = tempfile::tempdir().expect("create temp dir");
    let workspace_root = root.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");
    let modules_dir = workspace_root.join("node_modules");

    let mut config = Config::new();
    config.registry = "https://registry.test".to_string();
    config.store_dir = root
        .path()
        .join("materialization-store")
        .into();
    config.modules_dir = modules_dir.clone();
    config.install_state_dir = modules_dir.join(".pacquet");
    config.package_import_method = PackageImportMethod::Copy;
    config.offline = true;

    let package_key = key("from-shared-context", "1.0.0");
    let package_metadata = metadata_with_integrity(DUMMY_SHA512);
    let mut files = HashMap::new();
    for (path, content) in [
        ("package.json", br#"{"name":"from-shared-context","version":"1.0.0"}"#.as_slice()),
        ("index.js", b"module.exports = true\n".as_slice()),
    ] {
        let (_, digest) = config.store_dir
            .write_cas_file(content, false)
            .expect("write package file to materialization store");
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

    let context_store = StoreDir::new(root.path().join("context-store"));
    let index_key = store_index_key(DUMMY_SHA512, &package_key.without_peer().pkg_id());
    StoreIndex::open_in(&context_store)
        .expect("open context store index")
        .set(
            &index_key,
            &PackageFilesIndex {
                manifest: None,
                requires_build: Some(false),
                requires_prepare: None,
                algo: "sha512".to_string(),
                files,
                side_effects: None,
                remote_side_effects_quarantine: None,
                tar_integrity: None,
            },
        )
        .expect("seed context store index");
    let shared_index =
        StoreIndex::shared_readonly_in(&context_store).expect("open shared context store index");
    let verified_files_cache = SharedVerifiedFilesCache::default();

    let config = config.leak();
    let snapshots = HashMap::from([(package_key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(package_key.without_peer(), package_metadata)]);
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
    let progress_reported = SharedReportedProgressKeys::default();
    let (store_index_writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);
    let requester = workspace_root.to_string_lossy().into_owned();

    let output = CreateVirtualStore {
        fetching: crate::VirtualStoreFetchInputs {
            http_client: &pnpm_network::ThrottledClient::default(),
            store_index_writer: &store_index_writer,
            store_context: Some(CreateVirtualStoreStoreContext {
                index: Some(&shared_index),
                verified_files_cache: &verified_files_cache,
            }),
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
            known_requires_build: None,
            known_bins: None,
        },
        ctx: &crate::InstallContext {
            caches: crate::InstallCaches {
                logged_methods: &logged_methods,
                swap_lock: None,
                git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
                materialized_graph: std::sync::Arc::default(),
                dir_clone_cache: None,
            },

            linker: crate::ModuleLinkerContext {
                layout: &layout,
                kind: NodeLinker::Isolated,
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
    .expect("shared store context should satisfy the offline install");

    drop(store_index_writer);
    writer_task.await.expect("join store-index writer").expect("flush store-index writer");

    assert_eq!(output.requires_build_by_snapshot.get(&package_key), Some(&false));
    assert_eq!(output.materialized_snapshots.as_slice(), std::slice::from_ref(&package_key));
    let installed_body = layout
        .slot_dir(&package_key)
        .join("node_modules")
        .join("from-shared-context")
        .join("index.js");
    assert!(installed_body.is_file(), "warm package must be materialized: {installed_body:?}");
}
/// A snapshot the install materializes still has its store row
/// checked, so a row whose CAS blob is gone is re-fetched rather than
/// imported.
#[tokio::test]
async fn materialized_snapshot_with_a_missing_cas_blob_is_refetched() {
    let install = SeededStoreInstall::new(None);
    fs::remove_file(&install.body_blob).expect("remove the CAS blob behind index.js");

    let Err(error) = install.run().await else {
        panic!("offline re-fetch of the missing blob must fail");
    };
    let message = error.to_string();
    assert!(message.contains("offline mode"), "{message}");
}
/// `snapshot_deps_equal` is `true` when both `dependencies` and
/// `optionalDependencies` agree — matching upstream's `equals(...)`
/// pair. An absent map matches an empty map: pnpm canonicalises both
/// to `{}` via Ramda's `isEmpty`, so pacquet must too or warm
/// reinstalls would loop pointlessly when the lockfile drops the
/// optional-deps key.
#[test]
fn snapshot_deps_equal_treats_absent_and_empty_alike() {
    let absent = SnapshotEntry::default();
    let empty = SnapshotEntry {
        dependencies: Some(HashMap::new()),
        optional_dependencies: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(snapshot_deps_equal(&absent, &empty));
    assert!(snapshot_deps_equal(&empty, &absent));
}
/// A real diff on `dependencies` flips the result to `false`. Upstream
/// gates the skip on this comparison; if pacquet treated mismatched
/// child-version edges as "no change", a warm reinstall would silently
/// keep an outdated symlink layout when the lockfile bumped a
/// transitive.
#[test]
fn snapshot_deps_equal_distinguishes_different_dependency_values() {
    let entry_a = snapshot_with_dep("react", "17.0.2");
    let entry_b = snapshot_with_dep("react", "18.0.0");
    assert!(!snapshot_deps_equal(&entry_a, &entry_b));
}
#[test]
fn snapshot_deps_equal_distinguishes_different_optional_dependency_values() {
    let dep_ref: SnapshotDepRef = "1.0.0".parse().expect("parse dep ref");
    let entry_a = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(name("react"), dep_ref.clone())])),
        ..Default::default()
    };
    let entry_b = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(name("react-dom"), dep_ref)])),
        ..Default::default()
    };
    assert!(!snapshot_deps_equal(&entry_a, &entry_b));
}

#[test]
fn lookup_known_requires_build_matches_peer_variants_and_skips_patched_snapshots() {
    let base_key = key("foo", "1.0.0");
    let peer_key = key("foo", "1.0.0(react@18.0.0)");
    let patched_key = key("foo", "1.0.0(patch_hash=abc123)(react@18.0.0)");
    let known = HashMap::from([(base_key.clone(), false)]);

    assert_eq!(crate::lookup_known_requires_build(Some(&known), &base_key), Some(false));
    assert_eq!(crate::lookup_known_requires_build(Some(&known), &peer_key), Some(false));
    assert_eq!(crate::lookup_known_requires_build(Some(&known), &patched_key), None);
}

#[test]
fn needs_deferred_engine_probe_skips_when_every_snapshot_is_verified_plain() {
    use crate::{AllowBuildPolicy, materialization_plan::needs_deferred_engine_probe};
    use pnpm_config::{Config, NodeLinker};

    let config = Config::new();
    let policy = AllowBuildPolicy::default();
    let probe = |snapshots, known| {
        needs_deferred_engine_probe(&config, NodeLinker::Isolated, &policy, snapshots, known)
    };
    let pkg_a = key("foo", "1.0.0");
    let pkg_b = key("bar", "1.0.0(react@18.0.0)");
    let snapshots = HashMap::from([
        (pkg_a.clone(), SnapshotEntry::default()),
        (pkg_b.clone(), SnapshotEntry::default()),
    ]);
    let all_plain = HashMap::from([(pkg_a.clone(), false), (pkg_b.without_peer(), false)]);
    assert!(!probe(Some(&snapshots), Some(&all_plain)));

    let one_built = HashMap::from([(pkg_a.clone(), false), (pkg_b.without_peer(), true)]);
    assert!(probe(Some(&snapshots), Some(&one_built)));

    let missing_one = HashMap::from([(pkg_a, false)]);
    assert!(probe(Some(&snapshots), Some(&missing_one)));
    assert!(probe(Some(&snapshots), None));
}
