use super::{
    CreateVirtualDirBySnapshot, SlotForceInputs, optimistic_wire_method, remove_obsolete_child,
    slot_import_opts,
};
use pnpm_config::PackageImportMethod;
use pnpm_fs::force_symlink_dir;
use pnpm_lockfile::{PackageKey, PkgName, SnapshotEntry};
use pnpm_reporter::{LogEvent, PackageImportMethod as WireImportMethod, ProgressMessage, Reporter};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::tempdir;

pub struct LinkConcurrencyProbe {
    current: AtomicUsize,
    max: AtomicUsize,
    /// Total `enter()` calls over the probe's lifetime — one per
    /// [`CreateVirtualDirBySnapshot::run`] — for asserting how many
    /// link tasks executed, which `max_concurrent` cannot.
    total: AtomicUsize,
    wait_for_overlap: bool,
    wait_started: AtomicBool,
    mutex: Mutex<()>,
    condvar: Condvar,
}

/// How long the first linker waits for a second one to join it. Only has
/// to outlast the scheduling latency of a machine running the whole suite
/// in parallel — the verdict comes from `max_concurrent`, not from the
/// deadline.
const OVERLAP_TIMEOUT: Duration = Duration::from_secs(15);

impl LinkConcurrencyProbe {
    pub(crate) fn waiting_for_overlap() -> Self {
        Self { wait_for_overlap: true, ..Self::default() }
    }

    pub(crate) fn max_concurrent(&self) -> usize {
        self.max.load(Ordering::SeqCst)
    }

    pub(crate) fn total_entered(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }

    pub(super) fn enter(&self) -> LinkConcurrencyGuard<'_> {
        self.total.fetch_add(1, Ordering::SeqCst);
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        let mut max = self.max.load(Ordering::SeqCst);
        while current > max {
            match self.max.compare_exchange_weak(max, current, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    self.condvar.notify_all();
                    break;
                }
                Err(next) => max = next,
            }
        }

        if self.wait_for_overlap && current == 1 && !self.wait_started.swap(true, Ordering::SeqCst)
        {
            let guard = self.mutex.lock().expect("lock link-concurrency probe");
            let _ = self.condvar
                .wait_timeout_while(guard, OVERLAP_TIMEOUT, |()| {
                    self.max.load(Ordering::SeqCst) < 2
                })
                .expect("wait for overlapping link");
        }

        LinkConcurrencyGuard { probe: self }
    }
}

impl Default for LinkConcurrencyProbe {
    fn default() -> Self {
        Self {
            current: AtomicUsize::new(0),
            max: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            wait_for_overlap: false,
            wait_started: AtomicBool::new(false),
            mutex: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }
}

pub(super) struct LinkConcurrencyGuard<'a> {
    probe: &'a LinkConcurrencyProbe,
}

impl Drop for LinkConcurrencyGuard<'_> {
    fn drop(&mut self) {
        self.probe.current.fetch_sub(1, Ordering::SeqCst);
        self.probe.condvar.notify_all();
    }
}

/// `optimistic_wire_method` is the source of truth for the
/// configured-method → wire-method mapping the `imported` event
/// reports.
/// A future change to pacquet's `PackageImportMethod` set must
/// either extend this match or fail this test.
#[test]
fn optimistic_wire_method_reports_each_platforms_ladder_head() {
    #[cfg(target_os = "linux")]
    assert_eq!(optimistic_wire_method(PackageImportMethod::Auto), WireImportMethod::Hardlink);
    #[cfg(not(target_os = "linux"))]
    assert_eq!(optimistic_wire_method(PackageImportMethod::Auto), WireImportMethod::Clone);
    assert_eq!(optimistic_wire_method(PackageImportMethod::CloneOrCopy), WireImportMethod::Clone);
    assert_eq!(optimistic_wire_method(PackageImportMethod::Clone), WireImportMethod::Clone);
    assert_eq!(optimistic_wire_method(PackageImportMethod::Hardlink), WireImportMethod::Hardlink);
    assert_eq!(optimistic_wire_method(PackageImportMethod::Copy), WireImportMethod::Copy);
}

/// Driving with an empty `cas_paths` map exercises the success path
/// without hitting the network: `import_indexed_dir` mkdirs the empty
/// directory and returns Ok, then the imported emit fires.
#[tokio::test]
async fn run_emits_imported_event_after_import_indexed_dir() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }

    let dir = tempdir().expect("tempdir");
    let virtual_store_dir = dir.path().to_path_buf();
    let cas_paths: HashMap<String, std::path::PathBuf> = HashMap::new();
    let logged_methods = AtomicU8::new(0);
    let snapshot = SnapshotEntry::default();
    let package_key: PackageKey = "react@18.0.0".parse().expect("valid v9 snapshot key");

    EVENTS.lock().unwrap().clear();

    // `tokio::task::block_in_place` matches how the production
    // call-site (the `warm_work` closure in `CreateVirtualStore`)
    // drives this from inside a multi-thread runtime; a
    // `current_thread` runtime would panic on `block_in_place`,
    // but `#[tokio::test]` defaults to single-thread, so we run
    // `.run()` directly here. The function itself is sync — only
    // the caller's runtime flavor matters.
    let layout = crate::VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = crate::SkippedSnapshots::default();
    CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: &package_key,
            snapshot: &snapshot,
            skipped: &skipped,
            include_optional: true,
            removed_aliases: &[],
            symlink: true,
        },
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Hardlink,
            logged_methods: &logged_methods,
            requester: "/proj",
            isolate_mutable_sources: false,
        },
        source: crate::SlotImportSource {
            is_mutable: false,
            source_exists: true,
            force: false,
            build_marker: None,
            needs_build: false,
        },
        layout: &layout,
        cas_paths: &cas_paths,

        package_id: "react@18.0.0",

        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<RecordingReporter>()
    .expect("empty-cas-paths run should succeed");

    let captured = EVENTS.lock().unwrap();
    let imported = captured
        .iter()
        .find_map(|event| {
            let LogEvent::Progress(log) = event else { return None };
            let ProgressMessage::Imported { method, requester, to } = &log.message else {
                return None;
            };
            Some((*method, requester.clone(), to.clone()))
        });
    let (method, requester, to) =
        imported.unwrap_or_else(|| panic!("imported must fire; got {captured:?}"));
    assert_eq!(method, WireImportMethod::Hardlink);
    assert_eq!(requester, "/proj");
    // `to` is the per-package `node_modules/{name}` directory
    // inside the virtual store. The exact path depends on
    // `package_key.to_virtual_store_name()` and the temp dir
    // root, so spot-check the suffix via `Path::ends_with`
    // (component-based, so it works on Windows where `to` uses
    // backslashes too) instead of the full path.
    assert!(
        Path::new(&to).ends_with("react@18.0.0/node_modules/react"),
        "imported.to suffix must mirror the virtual-store layout; got {to}",
    );
}

/// A slot whose files a lifecycle script or a patch will still write must
/// not share inodes with its source, so it ignores `packageImportMethod`.
/// Every other slot keeps the configured method.
#[test]
fn needs_build_slots_ignore_the_configured_import_method() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }

    fn reported_import_method(
        method: PackageImportMethod,
        is_mutable: bool,
        needs_build: bool,
    ) -> WireImportMethod {
        let dir = tempdir().expect("tempdir");
        let layout = crate::VirtualStoreLayout::legacy(
            dir.path().to_path_buf(),
            pnpm_config::default_virtual_store_dir_max_length() as usize,
        );
        let skipped = crate::SkippedSnapshots::default();
        let snapshot = SnapshotEntry::default();
        let package_key: PackageKey = "react@18.0.0".parse().expect("valid snapshot key");
        let logged_methods = AtomicU8::new(0);

        EVENTS.lock().unwrap().clear();
        CreateVirtualDirBySnapshot {
            dependencies: crate::SnapshotDependencyLinks {
                package_key: &package_key,
                snapshot: &snapshot,
                skipped: &skipped,
                include_optional: true,
                removed_aliases: &[],
                symlink: true,
            },
            import: crate::PackageImportOptions {
                method,
                logged_methods: &logged_methods,
                requester: "/proj",
                isolate_mutable_sources: false,
            },
            source: crate::SlotImportSource {
                is_mutable,
                source_exists: true,
                force: false,
                build_marker: None,
                needs_build,
            },
            layout: &layout,
            cas_paths: &HashMap::new(),

            package_id: "react@18.0.0",

            dir_clone_cache: None,
            link_concurrency_probe: None,
        }
        .run::<RecordingReporter>()
        .expect("import should succeed");

        let captured = EVENTS.lock().unwrap();
        captured
            .iter()
            .find_map(|event| {
                let LogEvent::Progress(log) = event else { return None };
                let ProgressMessage::Imported { method, .. } = &log.message else { return None };
                Some(*method)
            })
            .unwrap_or_else(|| panic!("imported must fire; got {captured:?}"))
    }

    // An ordinary immutable package keeps the configured method.
    assert_eq!(
        reported_import_method(PackageImportMethod::Hardlink, false, false),
        WireImportMethod::Hardlink,
    );
    // So does a `file:` package that nothing will build.
    assert_eq!(
        reported_import_method(PackageImportMethod::Copy, true, false),
        WireImportMethod::Copy,
    );
    // A package that will be built imports with `clone-or-copy` instead,
    // whatever the configured method is.
    assert_eq!(
        reported_import_method(PackageImportMethod::Hardlink, false, true),
        WireImportMethod::Clone,
    );
    assert_eq!(
        reported_import_method(PackageImportMethod::Hardlink, true, true),
        WireImportMethod::Clone,
    );
}

/// The write-through the issue reports: a build script that rewrites a
/// shipped file inside its slot must not reach the source the slot was
/// imported from. On a filesystem where the hardlink tier works this fails
/// when the slot is hard-linked, and passes once a build forces
/// `clone-or-copy`.
#[test]
fn a_build_write_does_not_reach_the_import_source() {
    let dir = tempdir().expect("tempdir");
    let source_dir = dir.path().join("source");
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    let source_manifest = source_dir.join("package.json");
    let source_data = source_dir.join("data.txt");
    std::fs::write(
        &source_manifest,
        r#"{"name":"lib","version":"1.0.0","scripts":{"postinstall":"node build.js"}}"#,
    )
    .expect("write source manifest");
    std::fs::write(&source_data, "ORIGINAL").expect("write source data");

    let cas_paths = HashMap::from([
        ("package.json".to_string(), source_manifest),
        ("data.txt".to_string(), source_data.clone()),
    ]);
    let logged_methods = AtomicU8::new(0);
    let snapshot = SnapshotEntry::default();
    let package_key: PackageKey = "lib@file+packages+lib".parse().expect("valid snapshot key");
    let layout = crate::VirtualStoreLayout::legacy(
        dir.path().join("virtual-store"),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = crate::SkippedSnapshots::default();

    CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: &package_key,
            snapshot: &snapshot,
            skipped: &skipped,
            include_optional: true,
            removed_aliases: &[],
            symlink: true,
        },
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Hardlink,
            logged_methods: &logged_methods,
            requester: "/proj",
            isolate_mutable_sources: false,
        },
        source: crate::SlotImportSource {
            is_mutable: true,
            source_exists: true,
            force: false,
            build_marker: None,
            needs_build: true,
        },
        layout: &layout,
        cas_paths: &cas_paths,

        package_id: "lib@file+packages+lib",

        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>()
    .expect("import package that needs a build");

    // What the package's `postinstall` does to the file it ships.
    let slot_data = layout.slot_dir(&package_key).join("node_modules/lib/data.txt");
    std::fs::write(&slot_data, "MODIFIED by postinstall").expect("rewrite the slot's copy");

    assert_eq!(
        std::fs::read_to_string(&source_data).expect("read source data"),
        "ORIGINAL",
        "a build in the slot must not rewrite the source it was imported from",
    );
}

#[test]
fn run_imports_needs_build_marker_with_a_fresh_package() {
    let dir = tempdir().expect("tempdir");
    let cas_dir = dir.path().join("cas");
    std::fs::create_dir_all(&cas_dir).expect("create cas dir");
    let package_json = cas_dir.join("package.json");
    let marker_source = cas_dir.join("needs-build-marker");
    std::fs::write(&package_json, r#"{"name":"react","version":"18.0.0"}"#)
        .expect("write package manifest source");
    std::fs::write(&marker_source, "").expect("write marker source");

    let cas_paths = HashMap::from([("package.json".to_string(), package_json)]);
    let logged_methods = AtomicU8::new(0);
    let snapshot = SnapshotEntry::default();
    let package_key: PackageKey = "react@18.0.0".parse().expect("valid snapshot key");
    let layout = crate::VirtualStoreLayout::legacy(
        dir.path().join("virtual-store"),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = crate::SkippedSnapshots::default();

    CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: &package_key,
            snapshot: &snapshot,
            skipped: &skipped,
            include_optional: true,
            removed_aliases: &[],
            symlink: true,
        },
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Copy,
            logged_methods: &logged_methods,
            requester: "/proj",
            isolate_mutable_sources: false,
        },
        source: crate::SlotImportSource {
            is_mutable: false,
            source_exists: true,
            force: false,
            build_marker: Some(&marker_source),
            needs_build: true,
        },
        layout: &layout,
        cas_paths: &cas_paths,

        package_id: "react@18.0.0",

        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>()
    .expect("import package with build marker");

    let package_dir = layout.slot_dir(&package_key).join("node_modules/react");
    assert!(package_dir.join("package.json").exists());
    assert!(package_dir.join(crate::NEEDS_BUILD_MARKER).is_file());
}

#[test]
fn force_import_replaces_an_existing_package_at_the_same_snapshot_key() {
    let dir = tempdir().expect("tempdir");
    let cas_dir = dir.path().join("cas");
    std::fs::create_dir_all(&cas_dir).expect("create cas dir");
    let source = cas_dir.join("index.js");
    std::fs::write(&source, "module.exports = 'new'\n").expect("write CAS source");

    let package_key: PackageKey = "revision-pkg@1.0.0".parse().expect("package key");
    let layout = crate::VirtualStoreLayout::legacy(
        dir.path().join("virtual-store"),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let package_dir = layout.slot_dir(&package_key).join("node_modules/revision-pkg");
    std::fs::create_dir_all(&package_dir).expect("create existing package");
    std::fs::write(package_dir.join("index.js"), "module.exports = 'old'\n")
        .expect("write old package");
    std::fs::write(package_dir.join("removed.js"), "old only\n").expect("write removed file");

    CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: &package_key,
            snapshot: &SnapshotEntry::default(),
            skipped: &crate::SkippedSnapshots::default(),
            include_optional: true,
            removed_aliases: &[],
            symlink: true,
        },
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Copy,
            logged_methods: &AtomicU8::new(0),
            requester: "/proj",
            isolate_mutable_sources: false,
        },
        source: crate::SlotImportSource {
            is_mutable: false,
            source_exists: true,
            force: true,
            build_marker: None,
            needs_build: false,
        },
        layout: &layout,
        cas_paths: &HashMap::from([("index.js".to_string(), source)]),

        package_id: "revision-pkg@1.0.0",

        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>()
    .expect("replace package contents");

    assert_eq!(
        std::fs::read_to_string(package_dir.join("index.js")).expect("read replaced package"),
        "module.exports = 'new'\n",
    );
    assert!(!package_dir.join("removed.js").exists());
}

/// A snapshot key whose package name is a path traversal would become
/// the `<slot>/node_modules/<name>` extraction directory, escaping the
/// store. The guard rejects it before any package content is imported.
#[test]
fn run_rejects_traversal_package_name() {
    let dir = tempdir().expect("tempdir");
    let virtual_store_dir = dir.path().to_path_buf();
    let cas_paths: HashMap<String, std::path::PathBuf> = HashMap::new();
    let logged_methods = AtomicU8::new(0);
    let snapshot = SnapshotEntry::default();
    let package_key: PackageKey =
        "../../escaped@1.0.0".parse().expect("parse traversal snapshot key");

    let layout = crate::VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = crate::SkippedSnapshots::default();
    let result = CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: &package_key,
            snapshot: &snapshot,
            skipped: &skipped,
            include_optional: true,
            removed_aliases: &[],
            symlink: true,
        },
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Hardlink,
            logged_methods: &logged_methods,
            requester: "/proj",
            isolate_mutable_sources: false,
        },
        source: crate::SlotImportSource {
            is_mutable: false,
            source_exists: true,
            force: false,
            build_marker: None,
            needs_build: false,
        },
        layout: &layout,
        cas_paths: &cas_paths,

        package_id: "../../escaped@1.0.0",

        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>();

    assert!(
        matches!(result, Err(crate::CreateVirtualDirError::InvalidAlias(_))),
        "a traversal package name must be rejected before extraction; got {result:?}",
    );
}

/// A warm reinstall that drops a child dependency unlinks the stale
/// symlink (and its now-empty `@scope` directory) while leaving the
/// children it still depends on in place.
#[tokio::test]
async fn run_removes_obsolete_child_links() {
    use pnpm_reporter::SilentReporter;

    let dir = tempdir().expect("tempdir");
    let layout = crate::VirtualStoreLayout::legacy(
        dir.path().to_path_buf(),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let package_key: PackageKey = "react@18.0.0".parse().expect("valid snapshot key");
    let node_modules = layout.slot_dir(&package_key).join("node_modules");
    std::fs::create_dir_all(&node_modules).expect("create slot node_modules");

    let target = dir.path().join("target");
    std::fs::create_dir_all(&target).expect("create symlink target");
    for alias in ["is-positive", "@scope/old", "keep-me"] {
        force_symlink_dir(&target, &node_modules.join(alias)).expect("create stale child symlink");
    }

    let cas_paths: HashMap<String, std::path::PathBuf> = HashMap::new();
    let logged_methods = AtomicU8::new(0);
    let snapshot = SnapshotEntry::default();
    let skipped = crate::SkippedSnapshots::default();
    let removed_aliases =
        [PkgName::parse("is-positive").unwrap(), PkgName::parse("@scope/old").unwrap()];
    CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: &package_key,
            snapshot: &snapshot,
            skipped: &skipped,
            include_optional: true,
            removed_aliases: &removed_aliases,
            symlink: true,
        },
        import: crate::PackageImportOptions {
            method: PackageImportMethod::Hardlink,
            logged_methods: &logged_methods,
            requester: "/proj",
            isolate_mutable_sources: false,
        },
        source: crate::SlotImportSource {
            is_mutable: false,
            source_exists: true,
            force: false,
            build_marker: None,
            needs_build: false,
        },
        layout: &layout,
        cas_paths: &cas_paths,

        package_id: "react@18.0.0",

        dir_clone_cache: None,
        link_concurrency_probe: None,
    }
    .run::<SilentReporter>()
    .expect("run should succeed");

    assert!(!node_modules.join("is-positive").exists(), "obsolete child must be unlinked");
    assert!(!node_modules.join("@scope").exists(), "now-empty scope directory must be removed");
    assert!(
        node_modules
            .join("keep-me")
            .symlink_metadata()
            .is_ok(),
        "children not in removed_aliases must be left untouched",
    );
}

/// The traversal guard refuses to unlink an alias that resolves
/// outside the slot's `node_modules`. `PkgName` parsing accepts `..`,
/// so without the guard the join would escape the directory.
#[test]
fn remove_obsolete_child_skips_path_traversal() {
    let dir = tempdir().expect("tempdir");
    let node_modules = dir
        .path()
        .join("slot")
        .join("node_modules");
    std::fs::create_dir_all(&node_modules).expect("create node_modules");
    let sibling = dir.path().join("slot").join("sibling");
    std::fs::create_dir_all(&sibling).expect("create sibling dir");

    remove_obsolete_child(&node_modules, &PkgName::parse("..").unwrap())
        .expect("traversal alias is skipped, not an error");

    assert!(sibling.exists(), "a `..` alias must not delete a sibling of node_modules");
}

#[test]
fn remove_obsolete_child_skips_traversal_through_a_dependency_symlink() {
    let dir = tempdir().expect("tempdir");
    let node_modules = dir.path().join("slot/node_modules");
    let outside = dir.path().join("outside");
    let package = outside.join("package");
    let target = dir.path().join("target");
    for path in [&node_modules, &package, &target] {
        std::fs::create_dir_all(path).unwrap();
    }
    force_symlink_dir(&package, &node_modules.join("foo")).unwrap();
    let victim = outside.join("victim");
    force_symlink_dir(&target, &victim).unwrap();

    remove_obsolete_child(&node_modules, &PkgName::parse("foo/../victim").unwrap()).unwrap();

    assert!(victim.symlink_metadata().is_ok(), "cleanup must not unlink outside the slot");
}

/// A directory dependency's slot forces a reimport on every install, since
/// the source can change without the lockfile changing. But when the source
/// is a `publishConfig.directory` its own `prepare` script has not (re)built
/// yet, `DirectoryFetcher` tolerates the missing directory and comes back
/// with an empty file map; forcing a reimport from that would wipe an
/// already-materialized slot. `source_exists: false` must suppress the
/// force so the existing slot survives; an existing, genuinely empty source
/// must still force, since that reflects a real change.
#[test]
fn slot_import_opts_forces_only_when_a_mutable_source_exists() {
    let dir = tempdir().expect("tempdir");
    let layout = crate::VirtualStoreLayout::legacy(
        dir.path().to_path_buf(),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let missing_source = slot_import_opts(
        &layout,
        SlotForceInputs {
            interrupted_build: false,
            source_is_mutable: true,
            source_exists: false,
            force_import: false,
        },
    );
    assert!(!missing_source.force, "a missing mutable source must not force a reimport");

    let existing_source = slot_import_opts(
        &layout,
        SlotForceInputs {
            interrupted_build: false,
            source_is_mutable: true,
            source_exists: true,
            force_import: false,
        },
    );
    assert!(existing_source.force, "an existing mutable source must still force a reimport");
}

/// `interrupted_build` and `force_import` are independent reasons to force a
/// reimport, but neither may override the missing-source preservation above:
/// `pnpm install --force`, or a stale `.pnpm-needs-build` marker from an
/// interrupted build, must not wipe a slot whose mutable source is missing.
#[test]
fn slot_import_opts_missing_source_overrides_interrupted_build_and_force_import() {
    let dir = tempdir().expect("tempdir");
    let layout = crate::VirtualStoreLayout::legacy(
        dir.path().to_path_buf(),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let interrupted_build = slot_import_opts(
        &layout,
        SlotForceInputs {
            interrupted_build: true,
            source_is_mutable: true,
            source_exists: false,
            force_import: false,
        },
    );
    assert!(
        !interrupted_build.force,
        "interrupted_build must not force a reimport of a missing source",
    );

    let force_import = slot_import_opts(
        &layout,
        SlotForceInputs {
            interrupted_build: false,
            source_is_mutable: true,
            source_exists: false,
            force_import: true,
        },
    );
    assert!(!force_import.force, "force_import must not force a reimport of a missing source");
}
