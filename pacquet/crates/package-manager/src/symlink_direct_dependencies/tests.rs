use super::{SymlinkDirectDependencies, SymlinkDirectDependenciesError};
use crate::SkippedSnapshots;
use pacquet_config::Config;
use pacquet_lockfile::{Lockfile, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::{
    AddedRoot, DependencyType, LogEvent, Reporter, RootLog, RootMessage, SilentReporter,
};
use pacquet_testing_utils::fs::is_symlink_or_junction;
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tempfile::tempdir;

/// `pnpm:root added` fires once per direct dependency, after the
/// symlink under `node_modules/` has been created. The captured
/// payload must mirror pnpm's wire shape: `name` and `realName`
/// from the lockfile key, `version` from the resolved snapshot
/// spec, and `dependencyType` keyed off the originating
/// [`DependencyGroup`]. `prefix` is the install root, mirroring
/// pnpm's emit at
/// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L131>.
#[test]
fn emits_pnpm_root_added_per_direct_dependency() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir.clone();
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    // The symlink targets must exist for the test to work on
    // Windows: pacquet's `symlink_dir` falls back to junctions
    // there, and `junction::create` requires the target directory
    // to exist. On Unix `symlink` doesn't care, but creating the
    // dirs here keeps the test platform-uniform.
    for (store_name, real_name) in
        [("fastify@4.0.0", "fastify"), ("@pnpm.e2e+dev-dep@1.2.3", "@pnpm.e2e/dev-dep")]
    {
        let target = virtual_store_dir.join(store_name).join("node_modules").join(real_name);
        fs::create_dir_all(&target).expect("create symlink target");
    }

    // One prod and one dev dep so we can assert that `dependencyType`
    // tracks the originating group across the iteration order.
    let mut prod = ResolvedDependencyMap::new();
    prod.insert(
        "fastify".parse().expect("parse fastify pkg name"),
        ResolvedDependencySpec {
            specifier: "^4.0.0".to_string(),
            version: "4.0.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );
    let mut dev = ResolvedDependencyMap::new();
    dev.insert(
        "@pnpm.e2e/dev-dep".parse().expect("parse dev pkg name"),
        ResolvedDependencySpec {
            specifier: "^1.2.3".to_string(),
            version: "1.2.3".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );

    let project_snapshot = ProjectSnapshot {
        dependencies: Some(prod),
        dev_dependencies: Some(dev),
        ..ProjectSnapshot::default()
    };
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_snapshot);

    SymlinkDirectDependencies {
        config,
        layout: &crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        ),
        importers: &importers,
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Dev],
        workspace_root: &project_root,
        skipped: &SkippedSnapshots::default(),
        link_only: false,
        public_hoist_targets: None,
    }
    .run::<RecordingReporter>()
    .expect("symlink should succeed");

    // Both symlinks must land under `node_modules/` for the wire-
    // shape assertion below to be meaningful — an emit without the
    // matching FS effect would mask a real regression.
    let fastify_link = modules_dir.join("fastify");
    let dev_dep_link = modules_dir.join("@pnpm.e2e/dev-dep");
    assert!(
        is_symlink_or_junction(&fastify_link).unwrap(),
        "expected a symlink at {fastify_link:?}",
    );
    assert!(
        is_symlink_or_junction(&dev_dep_link).unwrap(),
        "expected a symlink at {dev_dep_link:?}",
    );

    let captured = EVENTS.lock().unwrap();
    let expected_prefix = project_root.to_string_lossy().into_owned();
    let added: Vec<&AddedRoot> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::Root(RootLog { message: RootMessage::Added { added, prefix }, .. }) => {
                assert_eq!(prefix, &expected_prefix);
                Some(added)
            }
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 2, "one pnpm:root added per direct dep");

    // par_iter doesn't pin order, so look up by name. Both entries
    // must carry their version, the matching dependency type, and
    // `realName == name` (pacquet's lockfile snapshots don't
    // preserve npm-alias keys at this layer).
    let fastify =
        added.iter().find(|added| added.name == "fastify").expect("fastify added event missing");
    assert_eq!(fastify.real_name, "fastify");
    assert_eq!(fastify.version.as_deref(), Some("4.0.0"));
    assert_eq!(fastify.dependency_type, Some(DependencyType::Prod));

    let dev = added
        .iter()
        .find(|added| added.name == "@pnpm.e2e/dev-dep")
        .expect("dev-dep added event missing");
    assert_eq!(dev.real_name, "@pnpm.e2e/dev-dep");
    assert_eq!(dev.version.as_deref(), Some("1.2.3"));
    assert_eq!(dev.dependency_type, Some(DependencyType::Dev));

    drop(dir);
}

/// A malformed importer snapshot can list the same package name
/// across multiple sections (e.g. both `dependencies` and
/// `optionalDependencies`). The dedup pass must collapse that to
/// one entry — first-wins per the caller-supplied
/// `dependency_groups` order — so we don't race two
/// `symlink_package` calls to the same `node_modules/<name>` and
/// emit two `pnpm:root added` events for the same dep.
#[test]
fn duplicate_dep_across_groups_collapses_to_one_entry() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let modules_dir = project_root.join("node_modules");
    let virtual_store_dir = modules_dir.join(".pacquet");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = modules_dir;
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    // Same name in `dependencies` and `optionalDependencies`. The
    // versions match here so the symlink target path is the same;
    // a real malformed lockfile that mismatched versions would also
    // collapse here, with first-wins picking the prod entry.
    let target = virtual_store_dir.join("fastify@4.0.0").join("node_modules").join("fastify");
    fs::create_dir_all(&target).expect("create symlink target");

    let mut prod = ResolvedDependencyMap::new();
    prod.insert(
        "fastify".parse().expect("parse fastify pkg name"),
        ResolvedDependencySpec {
            specifier: "^4.0.0".to_string(),
            version: "4.0.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );
    let mut optional = ResolvedDependencyMap::new();
    optional.insert(
        "fastify".parse().expect("parse fastify pkg name"),
        ResolvedDependencySpec {
            specifier: "^4.0.0".to_string(),
            version: "4.0.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );

    let project_snapshot = ProjectSnapshot {
        dependencies: Some(prod),
        optional_dependencies: Some(optional),
        ..ProjectSnapshot::default()
    };
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), project_snapshot);

    SymlinkDirectDependencies {
        config,
        layout: &crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        ),
        importers: &importers,
        // Prod first → first-wins gives `dependencyType: prod`.
        dependency_groups: [DependencyGroup::Prod, DependencyGroup::Optional],
        workspace_root: &project_root,
        skipped: &SkippedSnapshots::default(),
        link_only: false,
        public_hoist_targets: None,
    }
    .run::<RecordingReporter>()
    .expect("symlink should succeed");

    let captured = EVENTS.lock().unwrap();
    let added: Vec<&AddedRoot> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::Root(RootLog { message: RootMessage::Added { added, .. }, .. }) => {
                Some(added)
            }
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 1, "duplicate dep across groups must collapse to one emit");
    assert_eq!(added[0].name, "fastify");
    assert_eq!(added[0].dependency_type, Some(DependencyType::Prod));

    drop(dir);
}

/// A `workspace:*` dep in a sub-importer surfaces as `version:
/// link:<path>` in the lockfile. The symlink-direct-deps stage must
/// resolve that relative to the importer's `rootDir` and point the
/// `node_modules/<name>` symlink at the dependee project, NOT into
/// the virtual store. Mirrors upstream's
/// [`lockfileToDepGraph`](https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/types/src/index.ts)
/// branch for `link:` dependencies.
#[test]
fn cross_importer_link_dep_symlinks_to_sibling_rootdir() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let workspace_root = dir.path().to_path_buf();

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = workspace_root.join("node_modules/.pacquet");
    let config = config.leak();

    // Materialize the dependee project so the symlink target exists
    // before we create it. (Platform-uniform — same reasoning as the
    // single-importer test above.)
    let shared_dir = workspace_root.join("packages/shared");
    fs::create_dir_all(&shared_dir).unwrap();
    fs::write(shared_dir.join("package.json"), r#"{"name": "shared", "version": "1.0.0"}"#)
        .unwrap();

    let mut deps = ResolvedDependencyMap::new();
    deps.insert(
        "shared".parse().unwrap(),
        ResolvedDependencySpec {
            specifier: "workspace:*".to_string(),
            version: "link:../shared".parse().unwrap(),
        },
    );

    let mut importers = HashMap::new();
    importers.insert(
        "packages/web".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    );

    SymlinkDirectDependencies {
        config,
        layout: &crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        ),
        importers: &importers,
        dependency_groups: [DependencyGroup::Prod],
        workspace_root: &workspace_root,
        skipped: &SkippedSnapshots::default(),
        link_only: false,
        public_hoist_targets: None,
    }
    .run::<RecordingReporter>()
    .expect("symlink should succeed");

    // The symlink lives under the importer's `node_modules/` and
    // points at the sibling's `rootDir`, NOT into the virtual store.
    let symlink_path = workspace_root.join("packages/web/node_modules/shared");
    assert!(
        is_symlink_or_junction(&symlink_path).unwrap(),
        "expected a symlink at {symlink_path:?}",
    );

    // Confirm the reporter saw a `pnpm:root added` with the
    // resolved `link:` payload as `version` and the importer's
    // own `rootDir` as `prefix`.
    let captured = EVENTS.lock().unwrap();
    let added: Vec<(&str, &AddedRoot)> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::Root(RootLog { message: RootMessage::Added { added, prefix }, .. }) => {
                Some((prefix.as_str(), added))
            }
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 1);
    let (prefix, added) = added[0];
    let expected_prefix = workspace_root.join("packages/web").to_string_lossy().into_owned();
    assert_eq!(prefix, expected_prefix.as_str());
    assert_eq!(added.name, "shared");
    assert_eq!(added.version.as_deref(), Some("link:../shared"));

    drop(dir);
}

/// An empty `importers` map is a valid (if degenerate) lockfile —
/// nothing to link, no events emitted, no error. After per-importer
/// iteration landed for [#431], the old "missing root importer is a
/// hard error" contract is gone: each importer is now installed
/// independently, and a lockfile with zero importers simply produces
/// zero pnpm:root events. Pin this so the iteration loop never
/// regresses into requiring a root.
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
#[test]
fn empty_importers_is_a_no_op() {
    let dir = tempdir().unwrap();
    let project_root = dir.path().join("project");
    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = project_root.join("node_modules");
    config.virtual_store_dir = project_root.join("node_modules/.pacquet");
    let config = config.leak();

    let importers = HashMap::new();
    let layout = crate::VirtualStoreLayout::legacy(
        config.virtual_store_dir.clone(),
        config.virtual_store_dir_max_length as usize,
    );
    let result = SymlinkDirectDependencies {
        config,
        layout: &layout,
        importers: &importers,
        dependency_groups: [DependencyGroup::Prod],
        workspace_root: &project_root,
        skipped: &SkippedSnapshots::default(),
        link_only: false,
        public_hoist_targets: None,
    }
    .run::<SilentReporter>();

    assert!(result.is_ok(), "empty importers must not error: {result:?}");
    drop(dir);
}

/// Two importers under one workspace root each produce their own
/// `pnpm:root added` event with the importer's `rootDir` as the
/// event prefix. Mirrors upstream's per-project emit at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/direct-dep-linker/src/linkDirectDeps.ts#L131>.
#[test]
fn per_importer_prefix_in_pnpm_root_events() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = tempdir().unwrap();
    let workspace_root = dir.path().to_path_buf();
    let virtual_store_dir = workspace_root.join("node_modules/.pacquet");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    // Materialize the virtual-store targets each importer's symlink
    // points at — same precondition the single-importer test sets up.
    for store_name in ["fastify@4.0.0", "react@18.0.0"] {
        let real_name = store_name.split('@').next().unwrap();
        let target = virtual_store_dir.join(store_name).join("node_modules").join(real_name);
        fs::create_dir_all(&target).unwrap();
    }

    let mut alpha_deps = ResolvedDependencyMap::new();
    alpha_deps.insert(
        "fastify".parse().unwrap(),
        ResolvedDependencySpec {
            specifier: "^4.0.0".to_string(),
            version: "4.0.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );
    let mut beta_deps = ResolvedDependencyMap::new();
    beta_deps.insert(
        "react".parse().unwrap(),
        ResolvedDependencySpec {
            specifier: "^18.0.0".to_string(),
            version: "18.0.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );

    let mut importers = HashMap::new();
    importers.insert(
        "packages/alpha".to_string(),
        ProjectSnapshot { dependencies: Some(alpha_deps), ..ProjectSnapshot::default() },
    );
    importers.insert(
        "packages/beta".to_string(),
        ProjectSnapshot { dependencies: Some(beta_deps), ..ProjectSnapshot::default() },
    );

    SymlinkDirectDependencies {
        config,
        layout: &crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        ),
        importers: &importers,
        dependency_groups: [DependencyGroup::Prod],
        workspace_root: &workspace_root,
        skipped: &SkippedSnapshots::default(),
        link_only: false,
        public_hoist_targets: None,
    }
    .run::<RecordingReporter>()
    .unwrap();

    let captured = EVENTS.lock().unwrap();
    let added: Vec<(&str, &AddedRoot)> = captured
        .iter()
        .filter_map(|event| match event {
            LogEvent::Root(RootLog { message: RootMessage::Added { added, prefix }, .. }) => {
                Some((prefix.as_str(), added))
            }
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 2, "one event per importer's direct dep");

    let alpha_prefix = workspace_root.join("packages/alpha").to_string_lossy().into_owned();
    let beta_prefix = workspace_root.join("packages/beta").to_string_lossy().into_owned();
    let by_prefix: HashMap<&str, &AddedRoot> = added.iter().copied().collect();
    assert_eq!(by_prefix.get(alpha_prefix.as_str()).unwrap().name, "fastify");
    assert_eq!(by_prefix.get(beta_prefix.as_str()).unwrap().name, "react");

    drop(dir);
}

/// A malformed (or hostile) lockfile importer key that would resolve
/// outside the workspace root must error rather than silently
/// creating `node_modules` somewhere unrelated. `Path::join` discards
/// the workspace root when the right-hand side is absolute, and
/// allows `..` traversal otherwise, so the install layer enforces a
/// stricter shape.
#[test]
fn unsafe_importer_keys_error_before_filesystem_writes() {
    // Each case is an importer key that must produce
    // `UnsafeImporterPath` without touching the filesystem.
    let cases: &[&str] = &[
        "",                   // empty key (non-standard; `.` is the root)
        "/abs/path",          // absolute POSIX
        "..",                 // single parent
        "../sibling",         // traversal
        "packages/../escape", // mid-string traversal
        "C:/win",             // Windows drive prefix
        r"packages\web",      // backslash separator
    ];

    for &importer_id in cases {
        let dir = tempdir().unwrap();
        let workspace_root: PathBuf = dir.path().into();
        let mut config = Config::new();
        config.store_dir = dir.path().join("pacquet-store").into();
        config.modules_dir = workspace_root.join("node_modules");
        config.virtual_store_dir = workspace_root.join("node_modules/.pacquet");
        let config = config.leak();

        let mut importers = HashMap::new();
        importers.insert(importer_id.to_string(), ProjectSnapshot::default());

        let result = SymlinkDirectDependencies {
            config,
            layout: &crate::VirtualStoreLayout::legacy(
                config.virtual_store_dir.clone(),
                config.virtual_store_dir_max_length as usize,
            ),
            importers: &importers,
            dependency_groups: [DependencyGroup::Prod],
            workspace_root: &workspace_root,
            skipped: &SkippedSnapshots::default(),
            link_only: false,
            public_hoist_targets: None,
        }
        .run::<SilentReporter>();

        match result {
            Err(SymlinkDirectDependenciesError::UnsafeImporterPath { importer_id: id }) => {
                assert_eq!(id, importer_id, "expected the rejected key in the diagnostic");
            }
            other => panic!("expected UnsafeImporterPath for {importer_id:?}, got {other:?}"),
        }

        // The rejection happens before any per-importer work begins,
        // so nothing should have landed on disk. Guard that contract
        // by checking the workspace_root itself. We deliberately do
        // NOT inspect `workspace_root.parent()` here: on most CI hosts
        // the tempdir's parent is a shared system temp directory that
        // other tests (or unrelated processes) may have populated, so
        // an assertion there would be flaky for reasons unrelated to
        // the importer-id validator.
        assert!(
            !workspace_root.join("node_modules").exists(),
            "no node_modules should be created under workspace_root for {importer_id:?}",
        );
        drop(dir);
    }
}

/// A custom `modulesDir` (set via `pnpm-workspace.yaml`'s
/// `modulesDir` field) must propagate to every importer's per-project
/// dir, not stay hard-coded to `node_modules`. Otherwise the symlink
/// stage would write under `<importer>/node_modules/` while
/// `.modules.yaml` writing and bin linking (which still use
/// `config.modules_dir`) would target the configured name — two
/// inconsistent layouts for the same install. Mirrors pnpm where
/// `modulesDir` is one directory-name applied uniformly under every
/// importer's `rootDir`.
#[test]
fn custom_modules_dir_propagates_to_each_importer() {
    let dir = tempdir().unwrap();
    let workspace_root: PathBuf = dir.path().into();
    let virtual_store_dir = workspace_root.join("custom_modules/.pacquet");

    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    // `config.modules_dir`'s basename is the per-importer suffix.
    // Use a non-default name so a regression to the hard-coded
    // `node_modules` would fail the assertion below.
    config.modules_dir = workspace_root.join("custom_modules");
    config.virtual_store_dir = virtual_store_dir.clone();
    let config = config.leak();

    let target = virtual_store_dir.join("fastify@4.0.0").join("node_modules").join("fastify");
    fs::create_dir_all(&target).expect("create symlink target");

    let mut deps = ResolvedDependencyMap::new();
    deps.insert(
        "fastify".parse().unwrap(),
        ResolvedDependencySpec {
            specifier: "^4.0.0".to_string(),
            version: "4.0.0".parse::<pacquet_lockfile::PkgVerPeer>().unwrap().into(),
        },
    );
    let mut importers = HashMap::new();
    importers.insert(
        "packages/web".to_string(),
        ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
    );

    SymlinkDirectDependencies {
        config,
        layout: &crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        ),
        importers: &importers,
        dependency_groups: [DependencyGroup::Prod],
        workspace_root: &workspace_root,
        skipped: &SkippedSnapshots::default(),
        link_only: false,
        public_hoist_targets: None,
    }
    .run::<SilentReporter>()
    .expect("symlink should succeed");

    let expected = workspace_root.join("packages/web/custom_modules/fastify");
    assert!(
        is_symlink_or_junction(&expected).unwrap(),
        "expected per-importer symlink under the configured `modulesDir`: {expected:?}",
    );
    // The default `node_modules/` must NOT exist under the
    // importer's rootDir — that would mean the symlink stage
    // ignored the override.
    assert!(
        !workspace_root.join("packages/web/node_modules").exists(),
        "no `node_modules/` should be created when `modulesDir` overrides the suffix",
    );
    drop(dir);
}
