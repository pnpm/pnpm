use super::DirCloneCache;
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_reporter::{LogEvent, Reporter};
use std::{collections::HashMap, fs, path::PathBuf, sync::atomic::AtomicU8};
use tempfile::tempdir;

struct NullReporter;
impl Reporter for NullReporter {
    fn emit(_: &LogEvent) {}
}

#[test]
fn eligible_only_for_isolated_clone_capable_local_virtual_store() {
    let mut config = Config { enable_global_virtual_store: false, ..Config::default() };
    for method in
        [PackageImportMethod::Auto, PackageImportMethod::Clone, PackageImportMethod::CloneOrCopy]
    {
        config.package_import_method = method;
        assert_eq!(
            DirCloneCache::eligible(&config, NodeLinker::Isolated),
            cfg!(target_os = "macos"),
            "clone-capable method {method:?} must be eligible exactly on macOS",
        );
        assert!(!DirCloneCache::eligible(&config, NodeLinker::Hoisted));
    }
    for method in [PackageImportMethod::Hardlink, PackageImportMethod::Copy] {
        config.package_import_method = method;
        assert!(
            !DirCloneCache::eligible(&config, NodeLinker::Isolated),
            "explicit {method:?} promises an on-disk form a directory clone can't deliver",
        );
    }
    config.package_import_method = PackageImportMethod::Auto;
    config.enable_global_virtual_store = true;
    assert!(
        !DirCloneCache::eligible(&config, NodeLinker::Isolated),
        "a GVS install links straight out of the canonical slots and needs no projection",
    );
}

/// Build a cache whose canonical root is `links_root`, with one
/// snapshot so the layout precomputes its hashed slot.
#[cfg(target_os = "macos")]
fn cache_for_one_snapshot(links_root: &std::path::Path, key: &PackageKey) -> DirCloneCache {
    let config = Config {
        enable_global_virtual_store: false,
        package_import_method: PackageImportMethod::Clone,
        global_virtual_store_dir: links_root.to_path_buf(),
        ..Config::default()
    };
    let snapshots: HashMap<PackageKey, SnapshotEntry> =
        HashMap::from([(key.clone(), SnapshotEntry::default())]);
    DirCloneCache::build(&config, NodeLinker::Isolated, None, Some(&snapshots), None, None, None)
        .expect("eligible configuration must build a cache")
}

/// First import populates the canonical slot and clones it to the
/// target; a second import of the same package into a fresh target is
/// served entirely from the canonical slot.
#[cfg(target_os = "macos")]
#[test]
fn try_import_populates_canonical_slot_and_clones_it() {
    let dir = tempdir().expect("tempdir");
    let store = dir.path().join("cas");
    fs::create_dir_all(&store).expect("create fake CAS");
    let manifest_blob = store.join("manifest-blob");
    let lib_blob = store.join("lib-blob");
    fs::write(&manifest_blob, b"{\"name\":\"foo\"}").expect("write manifest blob");
    fs::write(&lib_blob, b"module.exports = 1").expect("write lib blob");
    let cas_paths: HashMap<String, PathBuf> = HashMap::from([
        ("package.json".to_string(), manifest_blob),
        ("lib/index.js".to_string(), lib_blob.clone()),
    ]);

    let key: PackageKey = "foo@1.0.0".parse().expect("valid snapshot key");
    let links_root = dir.path().join("links");
    let cache = cache_for_one_snapshot(&links_root, &key);
    let logged = AtomicU8::new(0);

    let first_target = dir.path().join("first/node_modules/foo");
    fs::create_dir_all(first_target.parent().unwrap()).expect("create slot node_modules");
    assert!(
        cache.try_import::<NullReporter>(
            &logged,
            PackageImportMethod::Clone,
            &key,
            &first_target,
            &cas_paths,
        ),
        "first import must be served by populating and cloning the canonical slot",
    );
    assert_eq!(
        fs::read(first_target.join("lib/index.js")).expect("cloned file"),
        b"module.exports = 1",
    );

    // The canonical slot exists under the links root with the
    // completion marker in place.
    let canonical_manifests = walk_files(&links_root)
        .into_iter()
        .filter(|path| path.ends_with("node_modules/foo/package.json"))
        .count();
    assert_eq!(canonical_manifests, 1, "one canonical slot for the one snapshot");

    // Deleting the CAS blob proves the second import reads only the
    // canonical slot.
    fs::remove_file(&lib_blob).expect("remove CAS blob");
    let second_target = dir.path().join("second/node_modules/foo");
    fs::create_dir_all(second_target.parent().unwrap()).expect("create slot node_modules");
    assert!(
        cache.try_import::<NullReporter>(
            &logged,
            PackageImportMethod::Clone,
            &key,
            &second_target,
            &cas_paths,
        ),
        "warm import must be served from the canonical slot alone",
    );
    assert_eq!(
        fs::read(second_target.join("lib/index.js")).expect("cloned file"),
        b"module.exports = 1",
    );
}

/// An existing dirent at the target belongs to `import_indexed_dir`'s
/// marker/repair logic, not the cache.
#[cfg(target_os = "macos")]
#[test]
fn try_import_declines_an_occupied_target() {
    let dir = tempdir().expect("tempdir");
    let key: PackageKey = "foo@1.0.0".parse().expect("valid snapshot key");
    let cache = cache_for_one_snapshot(&dir.path().join("links"), &key);
    let logged = AtomicU8::new(0);
    let target = dir.path().join("slot/node_modules/foo");
    fs::create_dir_all(&target).expect("pre-create target");
    assert!(!cache.try_import::<NullReporter>(
        &logged,
        PackageImportMethod::Clone,
        &key,
        &target,
        &HashMap::new(),
    ));
}

/// A snapshot key the layout never hashed (absent from the lockfile
/// walk) must not be served from a flat-name slot — flat names are not
/// content-addressed.
#[cfg(target_os = "macos")]
#[test]
fn try_import_declines_a_snapshot_without_a_hashed_slot() {
    let dir = tempdir().expect("tempdir");
    let key: PackageKey = "foo@1.0.0".parse().expect("valid snapshot key");
    let other: PackageKey = "bar@2.0.0".parse().expect("valid snapshot key");
    let cache = cache_for_one_snapshot(&dir.path().join("links"), &key);
    let logged = AtomicU8::new(0);
    let target = dir.path().join("slot/node_modules/bar");
    fs::create_dir_all(target.parent().unwrap()).expect("create slot node_modules");
    assert!(!cache.try_import::<NullReporter>(
        &logged,
        PackageImportMethod::Clone,
        &other,
        &target,
        &HashMap::new(),
    ));
}

#[cfg(target_os = "macos")]
fn walk_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}
