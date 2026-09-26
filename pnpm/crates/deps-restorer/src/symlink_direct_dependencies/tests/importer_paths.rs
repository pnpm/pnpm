use super::{
    super::validate_importer_id, Config, DependencyGroup, HashMap, LinkBinsOptions, PathBuf,
    ProjectSnapshot, SilentReporter, SkippedSnapshots, SymlinkDirectDependencies,
    SymlinkDirectDependenciesError, tempdir,
};

#[test]
fn validate_importer_id_accepts_root_and_relative_keys() {
    for id in [".", "packages/foo", "packages/foo/bar", "a.b/c"] {
        assert!(validate_importer_id(id).is_ok(), "expected {id:?} to be accepted");
    }
}

#[test]
fn validate_importer_id_rejects_escaping_keys() {
    // A lockfile importer key is joined onto the lockfile dir to read the
    // project's manifest; these forms would escape it, so they must be
    // rejected before any on-disk read.
    for id in
        ["", "..", "../foo", "packages/../../etc", "/abs/path", r"packages\foo", "C:/x", "C:x"]
    {
        assert!(validate_importer_id(id).is_err(), "expected {id:?} to be rejected");
    }
}

#[test]
fn validate_importer_id_rejects_non_canonical_aliases() {
    // Two distinct keys that resolve to the same directory would link
    // the same `node_modules` from two concurrent importer tasks; pnpm
    // only writes canonical relative keys, so every non-canonical form
    // is rejected outright.
    for id in ["./", "./foo", "packages/./app", "packages//app", "packages/app/", "foo/."] {
        assert!(validate_importer_id(id).is_err(), "expected {id:?} to be rejected");
    }
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
            context: crate::ImporterLinkContext {
                config,
                layout: &crate::VirtualStoreLayout::legacy(
                    config.virtual_store_dir.clone(),
                    config.virtual_store_dir_max_length as usize,
                ),
                workspace_root: &workspace_root,
                link_options: &LinkBinsOptions::default(),
            },
            graph: crate::ImporterDependencyGraph {
                importers: &importers,
                packages: None,
                skipped: &SkippedSnapshots::default(),
            },
            policy: crate::DirectLinkPolicy {
                public_hoist_targets: None,
                trusted_importer_ids: None,
                link_only: false,
            },

            dependency_groups: [DependencyGroup::Prod],

            package_manifests: None,
            requires_build_by_snapshot: None,
            scheduled_builds: None,
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

/// An importer id the caller declared as one of the install's own
/// projects bypasses the unsafe-path rejection — Bit's nested capsule
/// installs pass a project at `..`, whose `node_modules` lives outside
/// the lockfile dir by design. Ids NOT in the trusted set must still
/// be rejected.
#[test]
fn trusted_importer_id_outside_workspace_root_is_linked() {
    let dir = tempdir().unwrap();
    // The install root is a subdirectory; the trusted importer `..`
    // resolves to `dir` itself, above the workspace root.
    let workspace_root: PathBuf = dir.path().join("nested-install-root");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = workspace_root.join("node_modules");
    config.virtual_store_dir = workspace_root.join("node_modules/.pacquet");
    let config = config.leak();

    let mut importers = HashMap::new();
    importers.insert("..".to_string(), ProjectSnapshot::default());

    let trusted: std::collections::HashSet<String> = [".."].map(String::from).into();
    SymlinkDirectDependencies {
        context: crate::ImporterLinkContext {
            config,
            layout: &crate::VirtualStoreLayout::legacy(
                config.virtual_store_dir.clone(),
                config.virtual_store_dir_max_length as usize,
            ),
            workspace_root: &workspace_root,
            link_options: &LinkBinsOptions::default(),
        },
        graph: crate::ImporterDependencyGraph {
            importers: &importers,
            packages: None,
            skipped: &SkippedSnapshots::default(),
        },
        policy: crate::DirectLinkPolicy {
            public_hoist_targets: None,
            trusted_importer_ids: Some(&trusted),
            link_only: false,
        },

        dependency_groups: [DependencyGroup::Prod],

        package_manifests: None,
        requires_build_by_snapshot: None,
        scheduled_builds: None,
    }
    .run::<SilentReporter>()
    .expect("a declared project at `..` must be allowed");

    // An id missing from the trusted set keeps the strict rejection.
    let result = SymlinkDirectDependencies {
        context: crate::ImporterLinkContext {
            config,
            layout: &crate::VirtualStoreLayout::legacy(
                config.virtual_store_dir.clone(),
                config.virtual_store_dir_max_length as usize,
            ),
            workspace_root: &workspace_root,
            link_options: &LinkBinsOptions::default(),
        },
        graph: crate::ImporterDependencyGraph {
            importers: &importers,
            packages: None,
            skipped: &SkippedSnapshots::default(),
        },
        policy: crate::DirectLinkPolicy {
            public_hoist_targets: None,
            trusted_importer_ids: Some(&std::collections::HashSet::new()),
            link_only: false,
        },

        dependency_groups: [DependencyGroup::Prod],

        package_manifests: None,
        requires_build_by_snapshot: None,
        scheduled_builds: None,
    }
    .run::<SilentReporter>();
    assert!(
        matches!(result, Err(SymlinkDirectDependenciesError::UnsafeImporterPath { .. })),
        "an untrusted `..` id must still be rejected",
    );

    drop(dir);
}
