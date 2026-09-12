use super::{
    super::CreateVirtualStore, DUMMY_SHA512, SeededStoreInstall, directory_metadata,
    git_hosted_tarball_metadata, git_metadata, gvs_layout, key, metadata_with_integrity, name,
    slot_link, snapshot_with_dep, tarball_metadata_without_integrity,
};
use crate::{
    create_virtual_store::cache_keys::snapshot_cache_key,
    install_package_by_snapshot::host_platform_selector,
};
use pnpm_lockfile::{LockfileEntries, PackageKey, PkgName, SnapshotEntry};
use pnpm_reporter::SilentReporter;
use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::{Arc, atomic::AtomicU8},
};

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
        ctx: &crate::InstallContext {
            config,
            workspace_root: &workspace_root,
            requester: &requester,
            layout: &layout,
            node_linker: NodeLinker::Isolated,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
            dir_clone_cache: None,
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        entries: LockfileEntries { packages: Some(&packages), snapshots: Some(&snapshots) },
        current_entries: LockfileEntries::default(),
        store_index_writer: &store_index_writer,
        store_context: None,
        cas_prefetch: None,
        skipped: &skipped,
        include_optional_dependencies: true,
        supported_architectures: None,
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
/// A warm global-virtual-store slot is skipped without its store row's
/// CAS blobs being checked: nothing imports them, and the row still
/// feeds the build phase.
#[tokio::test]
async fn skipped_warm_slot_keeps_its_store_row_without_checking_cas_blobs() {
    let install = SeededStoreInstall::new(None);
    let first = install.run().await.expect("seeded store satisfies the offline install");
    assert_eq!(first.materialized_snapshots.as_slice(), std::slice::from_ref(&install.package_key));

    fs::remove_file(&install.body_blob).expect("remove the CAS blob behind index.js");

    let second = install.run().await.expect("an existing slot needs no CAS blob");
    assert!(
        second.materialized_snapshots.is_empty(),
        "the slot is current: {:?}",
        second.materialized_snapshots,
    );
    assert_eq!(second.requires_build_by_snapshot.get(&install.package_key), Some(&false));
}
/// A skipped slot's row is still checked when it carries a side-effects
/// overlay: the build phase's cache hit imports the overlay's base files
/// into the slot, so a row whose CAS blob is gone must not reach it.
#[tokio::test]
async fn skipped_warm_slot_with_a_side_effects_row_is_still_checked() {
    let install = SeededStoreInstall::new(Some(b"module.exports = 'built'\n"));
    let first = install.run().await.expect("seeded store satisfies the offline install");
    assert_eq!(first.requires_build_by_snapshot.get(&install.package_key), Some(&false));
    assert!(
        first.side_effects_maps_by_snapshot.contains_key(&install.package_key),
        "the seeded side-effects row must reach the build phase while its files verify",
    );

    fs::remove_file(&install.body_blob).expect("remove the CAS blob behind index.js");

    let second = install.run().await.expect("an existing slot needs no CAS blob");
    assert!(
        second.materialized_snapshots.is_empty(),
        "the slot is current: {:?}",
        second.materialized_snapshots,
    );
    assert!(
        !second.side_effects_maps_by_snapshot.contains_key(&install.package_key),
        "a row that failed its files check must not feed the build phase",
    );
    assert_eq!(
        second.requires_build_by_snapshot.get(&install.package_key),
        None,
        "a row that failed its files check must be dropped entirely",
    );
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
        ctx: &crate::InstallContext {
            config,
            workspace_root: &workspace_root,
            requester: &requester,
            layout: &layout,
            node_linker: NodeLinker::Isolated,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
            dir_clone_cache: None,
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        entries: LockfileEntries { packages: Some(&packages), snapshots: Some(&snapshots) },
        current_entries: LockfileEntries::default(),
        store_index_writer: &store_index_writer,
        store_context: None,
        cas_prefetch: None,
        skipped: &skipped,
        include_optional_dependencies: true,
        supported_architectures: None,
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

    let groups = crate::create_virtual_store::slot_linking::group_slots_by_dir(&slots, &layout);

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

    let groups = crate::create_virtual_store::slot_linking::group_slots_by_dir(&slots, &layout);

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

    let groups = crate::create_virtual_store::slot_linking::group_slots_by_dir(&slots, &layout);

    assert_eq!(groups.len(), 2, "non-GVS slots are unique per key; nothing may merge");
    assert!(groups.iter().all(|group| group.duplicates.is_empty()));
}
