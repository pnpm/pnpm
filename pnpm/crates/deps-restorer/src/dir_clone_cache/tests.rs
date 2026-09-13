use super::DirCloneCache;
use pnpm_config::{Config, NodeLinker, PackageImportMethod};

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

#[cfg(target_os = "macos")]
mod macos {
    use super::{Config, DirCloneCache, NodeLinker, PackageImportMethod};
    use crate::dir_clone_cache::EngineNameSource;
    use pnpm_lockfile::{PackageKey, SnapshotEntry};
    use pnpm_reporter::{LogEvent, Reporter};
    use std::{collections::HashMap, fs, path::PathBuf, sync::atomic::AtomicU8};
    use tempfile::tempdir;

    struct NullReporter;
    impl Reporter for NullReporter {
        fn emit(_: &LogEvent) {}
    }

    fn one_snapshot(key: &PackageKey) -> HashMap<PackageKey, SnapshotEntry> {
        HashMap::from([(key.clone(), SnapshotEntry::default())])
    }

    /// Build a cache whose canonical root is `links_root`, with
    /// `snapshots` so the layout precomputes their hashed slots. The
    /// virtual-store dir is pinned next to the links root so the
    /// capability probe in `build` clones within one volume — the
    /// default would point at the crate's working directory, which may
    /// live on another volume than the temp root.
    fn cache_for_snapshots<'a>(
        links_root: &std::path::Path,
        snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
        engine: EngineNameSource,
    ) -> DirCloneCache<'a> {
        let config = Config {
            enable_global_virtual_store: false,
            package_import_method: PackageImportMethod::Clone,
            global_virtual_store_dir: links_root.to_path_buf(),
            virtual_store_dir: links_root.with_file_name("probe-virtual-store"),
            ..Config::default()
        };
        DirCloneCache::build(
            &config,
            NodeLinker::Isolated,
            engine,
            Some(snapshots),
            None,
            None,
            None,
        )
        .expect("eligible configuration must build a cache")
    }

    /// Deleting the CAS blob between the two imports is what proves
    /// the second one reads only the canonical slot. The engine name
    /// arrives through a pending slot filled from another thread, so
    /// the first import also proves the lazy layout blocks until the
    /// deferred probe delivers.
    #[test]
    fn try_import_populates_canonical_slot_and_clones_it() {
        let dir = tempdir().expect("tempdir");
        let store = dir.path().join("cas");
        fs::create_dir_all(&store).expect("create fake CAS");
        let manifest_blob = store.join("manifest-blob");
        let lib_blob = store.join("lib-blob");
        fs::write(&manifest_blob, br#"{"name":"foo"}"#).expect("write manifest blob");
        fs::write(&lib_blob, b"module.exports = 1").expect("write lib blob");
        let cas_paths: HashMap<String, PathBuf> = HashMap::from([
            ("package.json".to_string(), manifest_blob),
            ("lib/index.js".to_string(), lib_blob.clone()),
        ]);

        let key: PackageKey = "foo@1.0.0".parse().expect("valid snapshot key");
        let links_root = dir.path().join("links");
        let snapshots = one_snapshot(&key);
        let engine_slot = std::sync::Arc::new(std::sync::OnceLock::new());
        let probe = std::thread::spawn({
            let engine_slot = std::sync::Arc::clone(&engine_slot);
            move || {
                std::thread::sleep(std::time::Duration::from_millis(30));
                let _ = engine_slot.set(Some("node-22".to_string()));
            }
        });
        let cache = cache_for_snapshots(
            &links_root,
            &snapshots,
            EngineNameSource::Pending(std::sync::Arc::clone(&engine_slot)),
        );
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

        let canonical_manifests = walk_files(&links_root)
            .into_iter()
            .filter(|path| path.ends_with("node_modules/foo/package.json"))
            .count();
        assert_eq!(canonical_manifests, 1, "one canonical slot for the one snapshot");
        let expected_slot = crate::VirtualStoreLayout::global(
            links_root.clone(),
            Config::default().virtual_store_dir_max_length as usize,
            Some("node-22"),
            Some(&snapshots),
            None,
            None,
            None,
        )
        .hashed_slot_dir(&key)
        .expect("hashed slot for the one snapshot");
        assert!(
            expected_slot.join("node_modules/foo/package.json").is_file(),
            "the canonical slot must be the one the delivered engine name selects: {expected_slot:?}",
        );

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
        probe.join().expect("probe thread");
    }

    /// An existing dirent at the target belongs to the marker/repair
    /// logic of [`fn@crate::import_indexed_dir`], not the cache.
    #[test]
    fn try_import_declines_an_occupied_target() {
        let dir = tempdir().expect("tempdir");
        let key: PackageKey = "foo@1.0.0".parse().expect("valid snapshot key");
        let snapshots = one_snapshot(&key);
        let cache = cache_for_snapshots(
            &dir.path().join("links"),
            &snapshots,
            EngineNameSource::Ready(None),
        );
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

    /// See [`crate::VirtualStoreLayout::hashed_slot_dir`] for why the
    /// flat-name fallback must never serve the cache.
    #[test]
    fn try_import_declines_a_snapshot_without_a_hashed_slot() {
        let dir = tempdir().expect("tempdir");
        let key: PackageKey = "foo@1.0.0".parse().expect("valid snapshot key");
        let other: PackageKey = "bar@2.0.0".parse().expect("valid snapshot key");
        let snapshots = one_snapshot(&key);
        let cache = cache_for_snapshots(
            &dir.path().join("links"),
            &snapshots,
            EngineNameSource::Ready(None),
        );
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
}
