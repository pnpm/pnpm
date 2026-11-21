use super::{
    HashMap,
    HoistOpts,
    Lockfile,
    ProjectSnapshot,
    assert_eq,
    hoist,
    lockfile_version,
};

#[test]
fn multi_importer_lockfile_emits_workspace_children() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), ProjectSnapshot::default());
    importers.insert("packages/foo".to_string(), ProjectSnapshot::default());
    importers.insert("packages/bar".to_string(), ProjectSnapshot::default());

    let lockfile = Lockfile {
        lockfile_version: lockfile_version(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("workspace hoist succeeds");
    let mut children: Vec<(String, String)> = result.dependencies
        .borrow()
        .iter()
        .map(|child| {
            (
                child.0.name.clone(),
                child.0.references
                    .borrow()
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_default(),
            )
        })
        .collect();
    children.sort();
    assert_eq!(
        children,
        vec![
            ("packages%2Fbar".to_string(), "workspace:packages/bar".to_string()),
            ("packages%2Ffoo".to_string(), "workspace:packages/foo".to_string()),
        ],
        "non-root importers are encoded as Workspace children",
    );
}

/// Tree membership does not depend on `hoist_workspace_packages`,
/// pnpm v11's `hoist()` attaches every importer unconditionally, and
/// the knob only controls root-level name links for the workspace
/// packages themselves. Gating membership on it silently dropped
/// every importer-only dependency from the hoisted install.
#[test]
fn hoist_workspace_packages_false_keeps_workspace_children() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), ProjectSnapshot::default());
    importers.insert("packages/foo".to_string(), ProjectSnapshot::default());

    let lockfile = Lockfile {
        lockfile_version: lockfile_version(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let opts = HoistOpts { hoist_workspace_packages: false, ..HoistOpts::default() };
    let result = hoist(&lockfile, &opts).expect("hoist succeeds");
    let children: Vec<String> = result.dependencies
        .borrow()
        .iter()
        .map(|child| child.0.name.clone())
        .collect();
    assert_eq!(
        children,
        vec!["packages%2Ffoo".to_string()],
        "non-root importers stay in the tree regardless of the knob",
    );
}
