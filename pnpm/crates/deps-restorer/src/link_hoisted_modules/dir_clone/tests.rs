use super::HoistedDirCloneCache;
use crate::{
    DirCloneCache,
    dir_clone_cache::EngineNameSource,
    link_hoisted_modules::{LinkHoistedModulesOpts, import_node, tests::make_node},
};
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_patching::PatchInfo;
use pnpm_reporter::SilentReporter;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};

fn metadata() -> PackageMetadata {
    serde_json::from_value(serde_json::json!({
        "resolution": { "integrity": "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" }
    }))
    .expect("package metadata")
}

fn cache<'a>(
    root: &Path,
    snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    packages: &'a HashMap<PackageKey, PackageMetadata>,
) -> DirCloneCache<'a> {
    DirCloneCache::build(
        &Config {
            enable_global_virtual_store: false,
            global_virtual_store_dir: root.join("links"),
            modules_dir: root.join("project/node_modules"),
            package_import_method: PackageImportMethod::Clone,
            ..Config::default()
        },
        NodeLinker::Hoisted,
        EngineNameSource::Ready(None),
        Some(snapshots),
        Some(packages),
        None,
        None,
    )
    .expect("directory clone cache")
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Requires macOS directory cloning")]
fn qualification_requires_immutable_build_free_unchanged_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let key: PackageKey = "foo@1.0.0".parse().expect("key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(key.clone(), metadata())]);
    let cache = cache(temp.path(), &snapshots, &packages);
    let flags = HashMap::from([(key.clone(), false)]);
    let eligible =
        HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), false)
            .expect("qualified cache");
    assert!(eligible.snapshots.contains(&key));
    assert!(
        HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), true)
            .is_none()
    );
    assert!(HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, None, false).is_none());
    let building = HashMap::from([(key.clone(), true)]);
    assert!(
        HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&building), false)
            .expect("cache")
            .snapshots
            .is_empty()
    );
    let changed = HashMap::from([(
        key.clone(),
        serde_json::from_value(serde_json::json!({
            "resolution": {"integrity": "sha256-AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}
        }))
        .expect("changed metadata"),
    )]);
    assert!(
        HoistedDirCloneCache::new(
            Some(&cache),
            Some(&packages),
            Some(&changed),
            Some(&flags),
            false
        )
        .expect("cache")
        .snapshots
        .is_empty()
    );
    for (resolution, eligible) in [
        (serde_json::json!({"type": "directory", "directory": "../foo"}), false),
        (
            serde_json::json!({"tarball": "file:../foo.tgz", "integrity": "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}),
            false,
        ),
        (
            serde_json::json!({"tarball": "https://example.com/foo.tgz", "integrity": "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}),
            true,
        ),
    ] {
        let packages = HashMap::from([(
            key.clone(),
            serde_json::from_value(serde_json::json!({"resolution": resolution}))
                .expect("package metadata"),
        )]);
        assert_eq!(
            HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), false)
                .expect("cache")
                .snapshots
                .contains(&key),
            eligible
        );
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Requires macOS directory cloning")]
fn local_tarball_imports_updated_content_despite_an_existing_canonical() {
    let temp = tempfile::tempdir().expect("tempdir");
    let key: PackageKey = "foo@file:../foo.tgz".parse().expect("key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(
        key.clone(),
        serde_json::from_value(serde_json::json!({"resolution": {
            "tarball": "file:../foo.tgz",
            "integrity": "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        }}))
        .expect("local tarball metadata"),
    )]);
    let cache = cache(temp.path(), &snapshots, &packages);
    let logged = AtomicU8::new(0);
    let original = temp.path().join("original");
    fs::write(&original, b"original").expect("original CAS");
    let first = temp.path().join("first/foo");
    fs::create_dir_all(first.parent().expect("parent")).expect("first directory");
    assert!(cache.try_import::<SilentReporter>(
        &logged,
        PackageImportMethod::Clone,
        &key,
        &first,
        &HashMap::from([("index.js".to_string(), original)])
    ));
    let flags = HashMap::from([(key, false)]);
    let cache = HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), false)
        .expect("cache");
    let node = make_node(
        "foo",
        "foo@file:../foo.tgz",
        "foo@file:../foo.tgz",
        temp.path().join("second/node_modules/foo"),
    );
    let updated = temp.path().join("updated");
    fs::write(&updated, b"updated").expect("updated CAS");
    let cas_index = HashMap::from([(
        node.pkg_id_with_patch_hash.clone(),
        Arc::new(HashMap::from([("index.js".to_string(), updated)])),
    )]);
    let opts = LinkHoistedModulesOpts {
        dir_clone_cache: Some(&cache),
        graph: &BTreeMap::new(),
        prev_graph: None,
        hierarchy: &BTreeMap::new(),
        cas_paths_by_pkg_id: &cas_index,
        import_method: PackageImportMethod::Clone,
        logged_methods: &logged,
        requester: "test",
        confine_root: temp.path(),
        link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
    };
    assert!(import_node::<SilentReporter>(&node, &opts).expect("fallback import"));
    assert_eq!(fs::read(node.dir.join("index.js")).expect("updated import"), b"updated");
    assert_eq!(fs::read(first.join("index.js")).expect("first import"), b"original");
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Requires macOS directory cloning")]
fn cloned_hoisted_aliases_reuse_canonical_content_and_remain_independent() {
    for (package, alias) in [("foo@1.0.0", "@alias/foo"), ("@source/foo@1.0.0", "foo")] {
        let temp = tempfile::tempdir().expect("tempdir");
        let key: PackageKey = package.parse().expect("key");
        let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
        let packages = HashMap::from([(key.clone(), metadata())]);
        let cache = cache(temp.path(), &snapshots, &packages);
        let flags = HashMap::from([(key, false)]);
        let cache =
            HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), false)
                .expect("cache");
        let blob = temp.path().join("manifest");
        fs::write(&blob, b"original").expect("CAS");
        let cas = HashMap::from([("package.json".to_string(), blob.clone())]);
        let logged = AtomicU8::new(0);
        let first =
            make_node(alias, package, package, temp.path().join("first/node_modules").join(alias));
        assert!(cache.try_import::<SilentReporter>(
            &first,
            &logged,
            PackageImportMethod::Clone,
            &cas
        ));
        fs::remove_file(&blob).expect("remove CAS to prove reuse");
        fs::write(first.dir.join("package.json"), b"project change").expect("edit first clone");
        let second = make_node(
            "foo",
            package,
            package,
            temp.path().join("second/node_modules/parent/node_modules/foo"),
        );
        assert!(cache.try_import::<SilentReporter>(
            &second,
            &logged,
            PackageImportMethod::Clone,
            &cas
        ));
        assert_eq!(fs::read(second.dir.join("package.json")).expect("second clone"), b"original");
        assert!(!cache.try_import::<SilentReporter>(
            &first,
            &logged,
            PackageImportMethod::Clone,
            &cas
        ));
        assert_eq!(
            fs::read(first.dir.join("package.json")).expect("preserved edit"),
            b"project change"
        );
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Requires macOS directory cloning")]
fn node_guards_decline_patches_bundles_and_present_targets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let key: PackageKey = "foo@1.0.0".parse().expect("key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(key.clone(), metadata())]);
    let cache = cache(temp.path(), &snapshots, &packages);
    let flags = HashMap::from([(key, false)]);
    let cache = HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), false)
        .expect("cache");
    let mut node =
        make_node("foo", "foo@1.0.0", "foo@1.0.0", temp.path().join("target/node_modules/foo"));
    let logged = AtomicU8::new(0);
    let blob = temp.path().join("manifest");
    fs::write(&blob, b"original").expect("CAS");
    let cas = HashMap::from([("package.json".to_string(), blob.clone())]);
    let control =
        make_node("foo", "foo@1.0.0", "foo@1.0.0", temp.path().join("control/node_modules/foo"));
    assert!(cache.try_import::<SilentReporter>(
        &control,
        &logged,
        PackageImportMethod::Clone,
        &cas
    ));
    node.present = true;
    assert!(!cache.try_import::<SilentReporter>(&node, &logged, PackageImportMethod::Clone, &cas));
    node.present = false;
    node.patch = Some(PatchInfo { hash: "hash".to_string(), patch_file_path: None });
    assert!(!cache.try_import::<SilentReporter>(&node, &logged, PackageImportMethod::Clone, &cas));
    node.patch = None;
    node.has_bundled_dependencies = true;
    assert!(!cache.try_import::<SilentReporter>(&node, &logged, PackageImportMethod::Clone, &cas));
    node.has_bundled_dependencies = false;
    let mut bundled = cas.clone();
    bundled.insert("node_modules/undeclared/index.js".to_string(), blob);
    assert!(!cache.try_import::<SilentReporter>(
        &node,
        &logged,
        PackageImportMethod::Clone,
        &bundled
    ));
    assert!(!node.dir.exists());
    assert!(cache.try_import::<SilentReporter>(&node, &logged, PackageImportMethod::Clone, &cas));
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "Requires macOS directory cloning")]
fn occupied_hoisted_parent_falls_back_without_losing_nested_dependencies() {
    let temp = tempfile::tempdir().expect("tempdir");
    let key: PackageKey = "foo@1.0.0".parse().expect("key");
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(key.clone(), metadata())]);
    let cache = cache(temp.path(), &snapshots, &packages);
    let flags = HashMap::from([(key, false)]);
    let cache = HoistedDirCloneCache::new(Some(&cache), Some(&packages), None, Some(&flags), false)
        .expect("cache");
    let node =
        make_node("foo", "foo@1.0.0", "foo@1.0.0", temp.path().join("project/node_modules/foo"));
    let blob = temp.path().join("manifest");
    fs::write(&blob, b"original").expect("CAS");
    let cas = HashMap::from([("package.json".to_string(), blob)]);
    let nested = node.dir.join("node_modules/child/index.js");
    fs::create_dir_all(nested.parent().expect("nested parent")).expect("nested directory");
    fs::write(&nested, b"nested dependency").expect("nested file");
    fs::write(node.dir.join("package.json"), b"stale").expect("stale manifest");
    let cas_index = HashMap::from([(node.pkg_id_with_patch_hash.clone(), Arc::new(cas))]);
    let opts = LinkHoistedModulesOpts {
        dir_clone_cache: Some(&cache),
        graph: &BTreeMap::new(),
        prev_graph: None,
        hierarchy: &BTreeMap::new(),
        cas_paths_by_pkg_id: &cas_index,
        import_method: PackageImportMethod::Clone,
        logged_methods: &AtomicU8::new(0),
        requester: "test",
        confine_root: temp.path(),
        link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
    };
    assert!(import_node::<SilentReporter>(&node, &opts).expect("fallback import"));
    assert_eq!(fs::read(node.dir.join("package.json")).expect("repaired manifest"), b"original");
    assert_eq!(fs::read(nested).expect("preserved dependency"), b"nested dependency");
}
