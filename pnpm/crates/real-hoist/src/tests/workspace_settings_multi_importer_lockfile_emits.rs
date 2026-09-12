use super::{
    HashMap, HoistOpts, Lockfile, ProjectSnapshot, assert_eq, build_hoist_ident_map, hoist,
    lockfile_version, result_node,
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
    let mut children: Vec<(String, String)> = result
        .dependencies
        .borrow()
        .iter()
        .map(|child| {
            (
                child.0.name.clone(),
                child.0.references.borrow().iter().next().cloned().unwrap_or_default(),
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

/// Tree membership does not depend on `hoist_workspace_packages` —
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
    let children: Vec<String> =
        result.dependencies.borrow().iter().map(|child| child.0.name.clone()).collect();
    assert_eq!(
        children,
        vec!["packages%2Ffoo".to_string()],
        "non-root importers stay in the tree regardless of the knob",
    );
}

/// A candidate whose name the root declares as its own peer dependency is
/// kept out of the ident map, even when reachable through the root's
/// non-peer descendants. Ports yarn's `!rootNode.peerNames.has(name)`
/// guard in `getHoistIdentMap`.
///
/// [`hoist`] always builds the `.` root with empty `peer_names`, so the
/// guard is unreachable from the public entry point. This drives
/// [`build_hoist_ident_map`] directly with a root that declares a peer —
/// the shape a per-importer hoisting root would take once those land.
#[test]
fn build_hoist_ident_map_skips_root_peer_names() {
    let react = result_node("react", "react@18.0.0", &[], vec![]);
    let app = result_node("app", "app@1.0.0", &[], vec![react]);
    let root = result_node(".", "", &["react"], vec![app]);

    let ident_map = build_hoist_ident_map(&root);
    dbg!(&ident_map);
    assert!(ident_map.contains_key("app"), "the non-peer child is recorded");
    assert!(
        !ident_map.contains_key("react"),
        "a name the root declares as a peer is skipped even when reachable transitively",
    );
}

/// When a *non-root* node declares one of its own children as a peer,
/// `add_dependent` records that child as a peer-dependent but does not
/// recurse into it (yarn's `entry.peerDependents.add` branch). The
/// child's own exclusive subtree is therefore never discovered.
///
/// Reachable from `hoist` only with an exotic lockfile where a package
/// lists the same name in both `dependencies` and `peerDependencies`;
/// driving [`build_hoist_ident_map`] directly is simpler and lets the
/// "subtree not walked" effect be asserted unambiguously.
#[test]
fn build_hoist_ident_map_records_node_peers_without_walking_their_subtree() {
    let scheduler = result_node("scheduler", "scheduler@1.0.0", &[], vec![]);
    let react = result_node("react", "react@18.0.0", &[], vec![scheduler]);
    let app = result_node("app", "app@1.0.0", &["react"], vec![react]);
    let root = result_node(".", "", &[], vec![app]);

    let ident_map = build_hoist_ident_map(&root);
    dbg!(&ident_map);
    assert!(ident_map.contains_key("app"), "the regular dep is recorded");
    assert!(ident_map.contains_key("react"), "the node's peer is still a candidate ident");
    assert!(
        !ident_map.contains_key("scheduler"),
        "a peer's exclusive subtree is not walked, so its child never enters the map",
    );
}
