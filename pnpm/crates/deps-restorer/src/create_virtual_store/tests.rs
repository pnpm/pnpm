use super::{
    CreateVirtualStore, CreateVirtualStoreStoreContext, emit_warm_snapshot_progress,
    integrity_equal, removed_child_aliases, snapshot_cache_key, snapshot_deps_equal,
};
use crate::install_package_by_snapshot::host_platform_selector;
use pnpm_lockfile::{
    GitResolution, LockfileResolution, PackageKey, PackageMetadata, PkgName, PkgVerPeer,
    RegistryResolution, SnapshotDepRef, SnapshotEntry, TarballResolution,
};
use pnpm_reporter::{LogEvent, ProgressMessage, Reporter, SilentReporter};
use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::{Arc, Mutex, atomic::AtomicU8},
};

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn metadata_with_integrity(integrity: &str) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity.parse().expect("parse integrity"),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn snapshot_with_dep(child: &str, ref_str: &str) -> SnapshotEntry {
    let dep_ref: SnapshotDepRef = ref_str.parse().expect("parse SnapshotDepRef");
    SnapshotEntry {
        dependencies: Some(HashMap::from([(name(child), dep_ref)])),
        ..Default::default()
    }
}

fn dep_map(children: &[&str]) -> Option<HashMap<PkgName, SnapshotDepRef>> {
    if children.is_empty() {
        return None;
    }
    // The ref value is irrelevant to `removed_child_aliases`; only the
    // alias keys matter. A bare version is the simplest valid ref.
    Some(children.iter().map(|child| (name(child), "1.0.0".parse().expect("ref"))).collect())
}

fn snapshot(deps: &[&str], optional: &[&str]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: dep_map(deps),
        optional_dependencies: dep_map(optional),
        ..Default::default()
    }
}

#[test]
fn removed_child_aliases_reports_dropped_children_only() {
    let self_name = name("host");
    let current = snapshot(&["kept", "dropped"], &["opt-dropped"]);
    let wanted = snapshot(&["kept", "added"], &[]);

    let mut removed: Vec<String> = removed_child_aliases(&current, &wanted, &self_name)
        .iter()
        .map(PkgName::to_string)
        .collect();
    removed.sort();

    assert_eq!(removed, vec!["dropped".to_string(), "opt-dropped".to_string()]);
}

#[test]
fn removed_child_aliases_excludes_self_and_unchanged_sets() {
    let self_name = name("host");
    // The slot lists itself as a dependency and is otherwise unchanged.
    let current = snapshot(&["host", "kept"], &[]);
    let wanted = snapshot(&["kept"], &[]);

    let removed = removed_child_aliases(&current, &wanted, &self_name);

    assert!(removed.is_empty(), "self and still-present children must not be removed: {removed:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_batch_links_slots_in_parallel() {
    use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
    use pnpm_config::{Config, NodeLinker, PackageImportMethod};
    use pnpm_store_dir::StoreIndexWriter;
    use pnpm_tarball::{CacheValue, MemCache, SharedReportedProgressKeys};

    if rayon::current_num_threads() < 2 {
        eprintln!(
            "skipping cold-batch concurrency assertion with rayon_threads={}",
            rayon::current_num_threads(),
        );
        return;
    }

    let root = tempfile::tempdir().expect("create temp dir");
    let workspace_root = root.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");
    let modules_dir = workspace_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");
    let store_dir = root.path().join("store");

    let mut config = Config::new();
    config.registry = "https://registry.test".to_string();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir.clone();
    config.package_import_method = PackageImportMethod::Copy;
    config.offline = true;
    let config = config.leak();

    let mut snapshots = HashMap::new();
    let mut packages = HashMap::new();
    let mem_cache = Arc::new(MemCache::default());
    for package_name in ["cold-a", "cold-b", "cold-c", "cold-d"] {
        let package_key = key(package_name, "1.0.0");
        let source_dir = workspace_root.join("prefetched").join(package_name);
        fs::create_dir_all(&source_dir).expect("create prefetched package dir");
        let manifest_path = source_dir.join("package.json");
        let manifest = if package_name == "cold-a" {
            format!(
                r#"{{"name":"{package_name}","version":"1.0.0","scripts":{{"postinstall":"node build.js"}}}}"#,
            )
        } else {
            format!(r#"{{"name":"{package_name}","version":"1.0.0"}}"#)
        };
        fs::write(&manifest_path, manifest).expect("write package manifest");
        let index_path = source_dir.join("index.js");
        fs::write(&index_path, "module.exports = true\n").expect("write package body");

        let cas_paths = HashMap::from([
            ("package.json".to_string(), manifest_path),
            ("index.js".to_string(), index_path),
        ]);
        mem_cache.insert(
            format!("https://registry.test/{package_name}/-/{package_name}-1.0.0.tgz"),
            Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(cas_paths)))),
        );

        snapshots.insert(package_key.clone(), SnapshotEntry::default());
        packages.insert(package_key.without_peer(), metadata_with_integrity(DUMMY_SHA512));
    }

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
    let probe =
        crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe::waiting_for_overlap();

    let output = CreateVirtualStore {
        http_client: &pnpm_network::ThrottledClient::default(),
        config,
        packages: Some(&packages),
        snapshots: Some(&snapshots),
        current_snapshots: None,
        current_packages: None,
        layout: &layout,
        logged_methods: &logged_methods,
        requester: &requester,
        store_index_writer: &store_index_writer,
        store_context: None,
        cas_prefetch: None,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        include_optional_dependencies: true,
        supported_architectures: None,
        workspace_root: &workspace_root,
        node_linker: NodeLinker::Isolated,
        dir_clone_cache: None,
        progress_reported: &progress_reported,
        tarball_mem_cache: Some(&mem_cache),
        custom_fetcher_session: None,
        planned_canonical_fetches: None,
        link_concurrency_probe: Some(&probe),
    }
    .run::<SilentReporter>()
    .await
    .expect("all-cold virtual-store creation should succeed from the mem cache");

    drop(store_index_writer);
    writer_task.await.expect("join store-index writer").expect("flush store-index writer");

    assert!(
        probe.max_concurrent() >= 2,
        "cold-batch slot linking must overlap; observed max_concurrent={} with rayon_threads={}",
        probe.max_concurrent(),
        rayon::current_num_threads(),
    );
    let cold_a = key("cold-a", "1.0.0");
    let cold_b = key("cold-b", "1.0.0");
    assert_eq!(output.requires_build_by_snapshot.get(&cold_a), Some(&true));
    assert_eq!(output.requires_build_by_snapshot.get(&cold_b), Some(&false));
    assert_eq!(
        output.materialized_snapshots.into_iter().collect::<HashSet<_>>(),
        HashSet::from([cold_a, cold_b, key("cold-c", "1.0.0"), key("cold-d", "1.0.0"),]),
    );
}

const DUMMY_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

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
    config.store_dir = root.path().join("materialization-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    config.package_import_method = PackageImportMethod::Copy;
    config.offline = true;

    let package_key = key("from-shared-context", "1.0.0");
    let package_metadata = metadata_with_integrity(DUMMY_SHA512);
    let mut files = HashMap::new();
    for (path, content) in [
        ("package.json", br#"{"name":"from-shared-context","version":"1.0.0"}"#.as_slice()),
        ("index.js", b"module.exports = true\n".as_slice()),
    ] {
        let (_, digest) = config
            .store_dir
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
        http_client: &pnpm_network::ThrottledClient::default(),
        config,
        packages: Some(&packages),
        snapshots: Some(&snapshots),
        current_snapshots: None,
        current_packages: None,
        layout: &layout,
        logged_methods: &logged_methods,
        requester: &requester,
        store_index_writer: &store_index_writer,
        cas_prefetch: None,
        store_context: Some(CreateVirtualStoreStoreContext {
            index: Some(&shared_index),
            verified_files_cache: &verified_files_cache,
        }),
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        include_optional_dependencies: true,
        supported_architectures: None,
        workspace_root: &workspace_root,
        node_linker: NodeLinker::Isolated,
        dir_clone_cache: None,
        progress_reported: &progress_reported,
        tarball_mem_cache: None,
        custom_fetcher_session: None,
        planned_canonical_fetches: None,
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

/// Under the global virtual store, peer variants hashing to one slot
/// directory must produce one link task through the whole
/// [`CreateVirtualStore::run`] pass — the probe's lifetime counter
/// distinguishes a real dedup from duplicates that happened to
/// serialize.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gvs_link_pass_materializes_shared_slot_once() {
    use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
    use pnpm_config::{Config, NodeLinker, PackageImportMethod};
    use pnpm_store_dir::StoreIndexWriter;
    use pnpm_tarball::{CacheValue, MemCache, SharedReportedProgressKeys};

    let root = tempfile::tempdir().expect("create temp dir");
    let workspace_root = root.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");
    let modules_dir = workspace_root.join("node_modules");
    let store_dir = root.path().join("store");

    let mut config = Config::new();
    config.registry = "https://registry.test".to_string();
    config.store_dir = store_dir.into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = modules_dir.join(".pacquet");
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = root.path().join("links");
    config.package_import_method = PackageImportMethod::Copy;
    config.offline = true;
    let config = config.leak();

    let mut snapshots = HashMap::new();
    let mut packages = HashMap::new();
    let mem_cache = Arc::new(MemCache::default());
    for package_name in ["shared", "solo"] {
        let source_dir = workspace_root.join("prefetched").join(package_name);
        fs::create_dir_all(&source_dir).expect("create prefetched package dir");
        let manifest_path = source_dir.join("package.json");
        fs::write(&manifest_path, format!(r#"{{"name":"{package_name}","version":"1.0.0"}}"#))
            .expect("write package manifest");
        let cas_paths = HashMap::from([("package.json".to_string(), manifest_path)]);
        mem_cache.insert(
            format!("https://registry.test/{package_name}/-/{package_name}-1.0.0.tgz"),
            Arc::new(tokio::sync::RwLock::new(CacheValue::Available(Arc::new(cas_paths)))),
        );
        packages.insert(
            key(package_name, "1.0.0").without_peer(),
            metadata_with_integrity(DUMMY_SHA512),
        );
    }
    // Two variants of `shared`, one bare and one peer-suffixed, with
    // identical dependency sets — the collision the fix is about.
    snapshots.insert(key("shared", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("shared", "1.0.0(peer@1.0.0)"), SnapshotEntry::default());
    snapshots.insert(key("solo", "1.0.0"), SnapshotEntry::default());

    let allow_build_policy = AllowBuildPolicy::default();
    let layout = VirtualStoreLayout::new(
        config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        Some(&allow_build_policy),
        None,
    );
    assert_eq!(
        layout.slot_dir(&key("shared", "1.0.0")),
        layout.slot_dir(&key("shared", "1.0.0(peer@1.0.0)")),
        "precondition: the variants must share one slot",
    );
    assert_ne!(
        layout.slot_dir(&key("shared", "1.0.0")),
        layout.slot_dir(&key("solo", "1.0.0")),
        "precondition: distinct packages must not share a slot",
    );

    let skipped = SkippedSnapshots::new();
    let logged_methods = AtomicU8::new(0);
    let progress_reported = SharedReportedProgressKeys::default();
    let (store_index_writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);
    let requester = workspace_root.to_string_lossy().into_owned();
    let probe = crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe::default();

    CreateVirtualStore {
        http_client: &pnpm_network::ThrottledClient::default(),
        config,
        packages: Some(&packages),
        snapshots: Some(&snapshots),
        current_snapshots: None,
        current_packages: None,
        layout: &layout,
        logged_methods: &logged_methods,
        requester: &requester,
        store_index_writer: &store_index_writer,
        store_context: None,
        cas_prefetch: None,
        allow_build_policy: &allow_build_policy,
        skipped: &skipped,
        include_optional_dependencies: true,
        supported_architectures: None,
        workspace_root: &workspace_root,
        node_linker: NodeLinker::Isolated,
        dir_clone_cache: None,
        progress_reported: &progress_reported,
        tarball_mem_cache: Some(&mem_cache),
        custom_fetcher_session: None,
        planned_canonical_fetches: None,
        link_concurrency_probe: Some(&probe),
    }
    .run::<SilentReporter>()
    .await
    .expect("global-virtual-store creation should succeed from the mem cache");

    drop(store_index_writer);
    writer_task.await.expect("join store-index writer").expect("flush store-index writer");

    assert_eq!(
        probe.total_entered(),
        2,
        "three snapshots over two unique slot directories must run exactly two link tasks",
    );
    let shared_manifest = layout
        .slot_dir(&key("shared", "1.0.0"))
        .join("node_modules")
        .join("shared")
        .join("package.json");
    assert!(shared_manifest.is_file(), "the shared slot must be materialized: {shared_manifest:?}");
}

/// `emit_warm_snapshot_progress` fires `resolved` then
/// `found_in_store` when no earlier fetch path already emitted the
/// package status. Both events carry the same identifiers — pnpm's
/// per-package counter relies on the pair to pin the tick to the right
/// package row.
#[test]
fn emits_resolved_then_found_in_store_when_not_progress_reported() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_warm_snapshot_progress::<RecordingReporter>("react@18.0.0", "/proj", false);

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Progress(r),
                LogEvent::Progress(f),
            ] if matches!(
                &r.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj"
            ) && matches!(
                &f.message,
                ProgressMessage::FoundInStore { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "warm-snapshot pair must be (Resolved, FoundInStore) with matching identifiers; got {captured:?}",
    );
}

/// When an earlier fetch path already emitted `fetched` or
/// `found_in_store`, the warm batch emits only `resolved` so the
/// package status is not double-counted. Regression guard for
/// <https://github.com/pnpm/pnpm/issues/12235>.
#[test]
fn emits_only_resolved_when_progress_reported() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_warm_snapshot_progress::<RecordingReporter>("react@18.0.0", "/proj", true);

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::Progress(r)] if matches!(
                &r.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj"
            ),
        ),
        "already-reported warm snapshot must report only Resolved; got {captured:?}",
    );
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
fn integrity_equal_matches_when_integrities_agree() {
    let entry_a = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let entry_b = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    assert!(integrity_equal(Some(&entry_a), Some(&entry_b)));
}

#[test]
fn integrity_equal_distinguishes_changed_integrities() {
    let entry_a = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let entry_b = metadata_with_integrity(
        "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
    );
    assert!(!integrity_equal(Some(&entry_a), Some(&entry_b)));
}

/// Missing metadata on either side (a malformed lockfile, or the
/// snapshot referring to a `packages:` entry that was dropped)
/// collapses to `None` on the integrity lookup. Both sides `None`
/// stays "equal" so a directory/git resolution pair (whose integrity
/// is `None`) doesn't trip a spurious re-fetch.
#[test]
fn integrity_equal_treats_none_pair_as_equal() {
    assert!(integrity_equal(None, None));
}

#[test]
fn integrity_equal_treats_one_sided_missing_as_unequal() {
    let with_integrity = metadata_with_integrity(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    assert!(!integrity_equal(None, Some(&with_integrity)));
    assert!(!integrity_equal(Some(&with_integrity), None));
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

fn git_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Git(GitResolution {
            repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
            commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
            integrity: None,
            path: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn git_hosted_tarball_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://codeload.github.com/foo/bar/tar.gz/f43f6a1cefff47fb361c88cf4b943fdbcaafe540"
                .to_string(),
            integrity: None,
            revision: None,
            git_hosted: Some(true),
            path: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

/// `Git` resolutions go through the warm batch under a
/// `gitHostedStoreIndexKey`-shaped key (`pkg_id\tbuilt|not-built`),
/// not under the integrity-based key. This is the read-side mirror
/// of what both fetchers write at install time — a drift between
/// the two would silently degrade every git-hosted re-install to
/// the cold path.
#[test]
fn snapshot_cache_key_for_git_resolution_uses_git_hosted_key() {
    let pkg = key("ts-pipe-compose", "0.2.1");
    let packages = HashMap::from([(pkg.clone(), git_metadata())]);

    let received = snapshot_cache_key(&pkg, &packages, false, &host_platform_selector())
        .expect("snapshot_cache_key must not error");
    assert_eq!(
        received.value,
        Some(format!("{pkg}\tbuilt")),
        "git resolutions must route through gitHostedStoreIndexKey",
    );
    assert!(received.is_git_hosted);
}

#[test]
fn snapshot_cache_key_for_git_hosted_tarball_uses_git_hosted_key() {
    let pkg = key("foo", "1.0.0");
    let packages = HashMap::from([(pkg.clone(), git_hosted_tarball_metadata())]);

    let received = snapshot_cache_key(&pkg, &packages, false, &host_platform_selector())
        .expect("snapshot_cache_key must not error");
    assert_eq!(
        received.value,
        Some(format!("{pkg}\tbuilt")),
        "git-hosted tarball resolutions must route through gitHostedStoreIndexKey",
    );
    assert!(received.is_git_hosted);
}

/// A plain remote tarball with no `integrity` is refused when the
/// fetch path reaches it, so it gets no warm key: the git-hosted key
/// shape `pickStoreIndexKey` would hand it is shared with every
/// package of the same id, and a row sitting there would materialize
/// the snapshot without the refusal ever running.
#[test]
fn snapshot_cache_key_for_a_refused_tarball_is_absent() {
    let pkg = key("foo", "1.0.0");
    let packages = HashMap::from([(pkg.clone(), tarball_metadata_without_integrity())]);

    let received = snapshot_cache_key(&pkg, &packages, false, &host_platform_selector())
        .expect("snapshot_cache_key must not error");
    assert_eq!(received.value, None, "a tarball the fetch path refuses must not warm-hit");
    assert!(!received.is_git_hosted);
}

fn tarball_metadata_without_integrity() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz".to_string(),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

/// Helpers for the `group_slots_by_dir` tests: a GVS layout over the
/// given snapshots/metadata, scoped to a lockfile dir so directory
/// resolutions hash the way a real project's do.
fn gvs_layout(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    lockfile_dir: &std::path::Path,
) -> crate::VirtualStoreLayout {
    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.virtual_store_dir = std::path::PathBuf::from("/tmp/proj/node_modules/.pnpm");
    config.global_virtual_store_dir = std::path::PathBuf::from("/tmp/store/links");
    let config = config.leak();
    crate::VirtualStoreLayout::new(
        config,
        Some("linux-x64-node22"),
        Some(snapshots),
        Some(packages),
        None,
        Some(lockfile_dir),
    )
}

fn directory_metadata(directory: &str) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution {
            directory: directory.to_string(),
        }),
        version: Some("1.0.0".to_string()),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn slot_link<'a>(
    snapshot_key: &'a PackageKey,
    snapshot: &'a SnapshotEntry,
    cas_paths: &'a HashMap<String, std::path::PathBuf>,
    removed_aliases: &'a [PkgName],
) -> super::SlotLink<'a> {
    super::SlotLink {
        snapshot_key,
        snapshot,
        cas_paths,
        warm_cache_key: None,
        source_is_mutable: true,
        force_import: false,
        needs_build_marker_source: None,
        dir_clone_cacheable: false,
        removed_aliases,
    }
}

/// Hash-equal peer variants of a directory dependency collapse into
/// one group, with the obsolete-alias cleanup covering both variants'
/// recorded removals.
#[test]
fn group_slots_by_dir_collapses_hash_equal_peer_variants() {
    let plain: PackageKey = "comp@file:packages/comp".parse().expect("parse plain key");
    let peered: PackageKey =
        "comp@file:packages/comp(peer@1.0.0)".parse().expect("parse peered key");

    let mut snapshots = HashMap::new();
    snapshots.insert(plain.clone(), SnapshotEntry::default());
    snapshots.insert(peered.clone(), SnapshotEntry::default());
    let mut packages = HashMap::new();
    packages.insert(plain.clone(), directory_metadata("packages/comp"));

    let layout = gvs_layout(&snapshots, &packages, std::path::Path::new("/home/user/project"));
    assert_eq!(
        layout.slot_dir(&plain),
        layout.slot_dir(&peered),
        "precondition: hash-equal variants must resolve to one slot",
    );

    let snapshot = SnapshotEntry::default();
    let cas_paths = HashMap::new();
    let removed_plain = [name("dropped-a")];
    let removed_peered = [name("dropped-a"), name("dropped-b")];
    let slots = [
        slot_link(&plain, &snapshot, &cas_paths, &removed_plain),
        slot_link(&peered, &snapshot, &cas_paths, &removed_peered),
    ];

    let groups = super::group_slots_by_dir(&slots, &layout);

    assert_eq!(groups.len(), 1, "hash-equal variants must share one link task");
    assert_eq!(groups[0].duplicates.len(), 1);
    let mut merged: Vec<String> =
        groups[0].removed_aliases().iter().map(PkgName::to_string).collect();
    merged.sort();
    assert_eq!(
        merged,
        vec!["dropped-a".to_string(), "dropped-b".to_string()],
        "cleanup must cover aliases recorded against either variant",
    );
}

/// Peer variants whose subtrees differ hash to different slots and must
/// keep their own link tasks.
#[test]
fn group_slots_by_dir_keeps_diverging_peer_variants_apart() {
    let plain: PackageKey = "comp@file:packages/comp".parse().expect("parse plain key");
    let peered: PackageKey =
        "comp@file:packages/comp(peer@1.0.0)".parse().expect("parse peered key");
    let child: PackageKey = key("leaf", "1.0.0");

    let mut snapshots = HashMap::new();
    snapshots.insert(plain.clone(), SnapshotEntry::default());
    // The peered variant resolves an extra child, so its recursive
    // hash — and therefore its slot — must diverge from the plain one.
    snapshots.insert(peered.clone(), snapshot_with_dep("leaf", "1.0.0"));
    snapshots.insert(child.clone(), SnapshotEntry::default());
    let mut packages = HashMap::new();
    packages.insert(plain.clone(), directory_metadata("packages/comp"));
    packages.insert(child, metadata_with_integrity(DUMMY_SHA512));

    let layout = gvs_layout(&snapshots, &packages, std::path::Path::new("/home/user/project"));
    assert_ne!(
        layout.slot_dir(&plain),
        layout.slot_dir(&peered),
        "precondition: diverging subtrees must resolve to distinct slots",
    );

    let snapshot = SnapshotEntry::default();
    let cas_paths = HashMap::new();
    let slots = [
        slot_link(&plain, &snapshot, &cas_paths, &[]),
        slot_link(&peered, &snapshot, &cas_paths, &[]),
    ];

    let groups = super::group_slots_by_dir(&slots, &layout);

    assert_eq!(groups.len(), 2, "distinct slots must keep distinct link tasks");
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
    assert!(groups.iter().all(|group| group.merged_removed_aliases.is_none()));
}

/// Without the global virtual store, `slot_dir` embeds the full
/// peer-suffixed key, so grouping never merges anything and the link
/// pass matches the ungrouped behavior exactly.
#[test]
fn group_slots_by_dir_is_identity_without_gvs() {
    let plain: PackageKey = "comp@file:packages/comp".parse().expect("parse plain key");
    let peered: PackageKey =
        "comp@file:packages/comp(peer@1.0.0)".parse().expect("parse peered key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = false;
    config.virtual_store_dir = std::path::PathBuf::from("/tmp/proj/node_modules/.pnpm");
    let config = config.leak();
    let layout = crate::VirtualStoreLayout::new(config, None, None, None, None, None);

    let snapshot = SnapshotEntry::default();
    let cas_paths = HashMap::new();
    let slots = [
        slot_link(&plain, &snapshot, &cas_paths, &[]),
        slot_link(&peered, &snapshot, &cas_paths, &[]),
    ];

    let groups = super::group_slots_by_dir(&slots, &layout);

    assert_eq!(groups.len(), 2, "non-GVS slots are unique per key; nothing may merge");
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}
