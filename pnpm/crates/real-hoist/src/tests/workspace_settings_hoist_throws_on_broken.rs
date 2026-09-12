use super::{
    BTreeSet, HashMap, HoistError, HoistOpts, HoisterResult, Lockfile, ProjectSnapshot, Rc,
    ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry, assert_eq, dep_key, empty_lockfile,
    hoist, lockfile_version, pkg_metadata_with_peer, pkg_name, resolved_dep, ver_peer,
};

#[test]
fn hoist_throws_on_broken_lockfile() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("foo"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

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

    let err = hoist(&lockfile, &HoistOpts::default()).expect_err("missing snapshot should error");
    let HoistError::LockfileMissingDependency { pkg_key } = err;
    assert_eq!(pkg_key, "foo@1.0.0");
}

#[test]
fn one_transitive_dep_hoists_to_root() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("happy hoist should succeed");
    assert_eq!(result.name, ".");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b"], "both a and b sit at root: {result:#?}");
    let dep_a = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "a").unwrap().0);
    assert!(dep_a.dependencies.borrow().is_empty(), "a's b moved to root: {dep_a:#?}");
}

#[test]
fn diamond_dep_hoists_once_to_root() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    let mut c_deps = HashMap::new();
    c_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("c", "1.0.0"),
        SnapshotEntry { dependencies: Some(c_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b", "c"], "diamond flattens at root: {result:#?}");
    let dep_a = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "a").unwrap().0);
    let dep_c = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "c").unwrap().0);
    assert!(dep_a.dependencies.borrow().is_empty(), "a stripped of its b: {dep_a:#?}");
    assert!(dep_c.dependencies.borrow().is_empty(), "c stripped of its b: {dep_c:#?}");

    let mut b_ptrs: std::collections::HashSet<*const HoisterResult> =
        std::collections::HashSet::new();
    let mut stack: Vec<Rc<HoisterResult>> =
        root_children.iter().map(|dep| Rc::clone(&dep.0)).collect();
    let mut walked: std::collections::HashSet<*const HoisterResult> =
        std::collections::HashSet::new();
    while let Some(node) = stack.pop() {
        if !walked.insert(Rc::as_ptr(&node)) {
            continue;
        }
        if node.name == "b" {
            b_ptrs.insert(Rc::as_ptr(&node));
        }
        for d in node.dependencies.borrow().iter() {
            stack.push(Rc::clone(&d.0));
        }
    }
    assert_eq!(b_ptrs.len(), 1, "exactly one `b` allocation across the entire result graph");
}

#[test]
fn peer_check_uses_post_hoist_ancestor_path_not_queue_time_path() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("app"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("react"), resolved_dep("18.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut app_deps = HashMap::new();
    app_deps.insert(pkg_name("react"), SnapshotDepRef::Plain(ver_peer("17.0.0")));
    app_deps.insert(pkg_name("mid"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("app", "1.0.0"),
        SnapshotEntry { dependencies: Some(app_deps), ..SnapshotEntry::default() },
    );
    let mut mid_deps = HashMap::new();
    mid_deps.insert(pkg_name("terminal"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("mid", "1.0.0"),
        SnapshotEntry { dependencies: Some(mid_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("react", "17.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("react", "18.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("terminal", "1.0.0"), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(dep_key("terminal", "1.0.0").without_peer(), pkg_metadata_with_peer("react"));

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
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["app", "mid", "react", "terminal"],
        "mid and terminal hoist freely: {result:#?}",
    );
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    let app_deps = app.dependencies.borrow();
    let app_names: Vec<&str> = app_deps.iter().map(|dep| dep.0.name.as_str()).collect();
    assert_eq!(app_names, ["react"], "app retains conflicting react@17: {app_names:?}");
    drop(app_deps);
    let mid = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "mid").unwrap().0);
    assert!(mid.dependencies.borrow().is_empty(), "mid stripped of terminal: {mid:#?}");
}

#[test]
fn peer_constrained_node_hoists_when_ancestor_and_root_agree() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("app"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("react"), resolved_dep("18.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut app_deps = HashMap::new();
    app_deps.insert(pkg_name("widget"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    app_deps.insert(pkg_name("react"), SnapshotDepRef::Plain(ver_peer("18.0.0")));
    snapshots.insert(
        dep_key("app", "1.0.0"),
        SnapshotEntry { dependencies: Some(app_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("widget", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("react", "18.0.0"), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(dep_key("widget", "1.0.0").without_peer(), pkg_metadata_with_peer("react"));

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
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("peer-aware hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["app", "react", "widget"], "widget hoists past app: {result:#?}");
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    assert!(
        app.dependencies.borrow().is_empty(),
        "app stripped of its hoisted widget + dedup'd react: {app:#?}",
    );
}

/// Iteration order over `app`'s children is alphabetical, so
/// `widget` is visited *before* `x` in round 1 — that ordering is
/// what forces the refuse-then-reconsider path the test pins.
#[test]
fn multi_round_unlocks_peer_friendly_hoist_after_blocker_moves() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("app"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    // Root carries no x of its own, so the only x in scope before
    // hoisting is the one under app — exactly the multi-round trigger.
    let mut snapshots = HashMap::new();
    let mut app_deps = HashMap::new();
    app_deps.insert(pkg_name("widget"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    app_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("app", "1.0.0"),
        SnapshotEntry { dependencies: Some(app_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("widget", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("x", "1.0.0"), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(dep_key("widget", "1.0.0").without_peer(), pkg_metadata_with_peer("x"));

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
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("multi-round should converge");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["app", "widget", "x"],
        "widget hoists in round 2 after x clears app in round 1: {result:#?}",
    );
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    assert!(app.dependencies.borrow().is_empty(), "app stripped after multi-round: {app:#?}");
}

#[test]
fn hoisting_limits_border_keeps_descendants_nested() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let mut blocked = BTreeSet::new();
    blocked.insert("a".to_string());
    let mut opts = HoistOpts::default();
    opts.hoisting_limits.insert(".@".to_string(), blocked);

    let result = hoist(&lockfile, &opts).expect("hoist with limits should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a"], "border node a sits at root; b did not flatten: {result:#?}");
    let dep_a = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "a").unwrap().0);
    let a_deps = dep_a.dependencies.borrow();
    let a_names: Vec<&str> = a_deps.iter().map(|dep| dep.0.name.as_str()).collect();
    assert_eq!(a_names, ["b"], "b stays nested under the border a: {a_names:?}");
}

#[test]
fn hoisting_limits_border_keeps_all_descendants_nested() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    a_deps.insert(pkg_name("c"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    a_deps.insert(pkg_name("d"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("c", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("d", "1.0.0"), SnapshotEntry::default());

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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let mut blocked = BTreeSet::new();
    blocked.insert("a".to_string());
    let mut opts = HoistOpts::default();
    opts.hoisting_limits.insert(".@".to_string(), blocked);

    let result = hoist(&lockfile, &opts).expect("hoist with limits should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a"], "only the border a sits at root: {result:#?}");
    let dep_a = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "a").unwrap().0);
    let a_deps = dep_a.dependencies.borrow();
    let mut a_names: Vec<&str> = a_deps.iter().map(|dep| dep.0.name.as_str()).collect();
    a_names.sort_unstable();
    assert_eq!(
        a_names,
        ["b", "c", "d"],
        "all of a's deps stay nested under the border: {a_names:?}",
    );
}

#[test]
fn hoisting_limits_keyed_on_unrelated_importer_is_inert() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());

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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let mut blocked = BTreeSet::new();
    blocked.insert("b".to_string());
    let mut opts = HoistOpts::default();
    // Wrong key — `packages/foo@workspace:packages/foo`, not `.@`.
    opts.hoisting_limits.insert("packages/foo@workspace:packages/foo".to_string(), blocked);

    let result = hoist(&lockfile, &opts).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b"], "limits keyed elsewhere don't affect root hoist: {result:#?}");
}

#[test]
fn nested_hoist_uses_the_nested_root_locator() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), ProjectSnapshot::default());
    let mut workspace_deps = ResolvedDependencyMap::new();
    workspace_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        "packages/foo".to_string(),
        ProjectSnapshot { dependencies: Some(workspace_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    let mut b_deps = HashMap::new();
    b_deps.insert(pkg_name("c"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("b", "1.0.0"),
        SnapshotEntry { dependencies: Some(b_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("c", "1.0.0"), SnapshotEntry::default());

    let mut opts = HoistOpts::default();
    opts.hoisting_limits.insert(".@".to_string(), BTreeSet::from(["packages%2Ffoo".to_string()]));
    opts.hoisting_limits.insert(
        "packages%2Ffoo@workspace:packages/foo".to_string(),
        BTreeSet::from(["b".to_string()]),
    );

    let result = hoist(
        &Lockfile {
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
            snapshots: Some(snapshots),
            time: None,
            extra: pnpm_lockfile::LockfileExtra::default(),
        },
        &opts,
    )
    .expect("nested hoist with importer limits should succeed");

    let root_children = result.dependencies.borrow();
    let workspace = Rc::clone(
        &root_children
            .iter()
            .find(|dep| dep.0.name == "packages%2Ffoo")
            .expect("workspace stays at the virtual root")
            .0,
    );
    let workspace_deps = workspace.dependencies.borrow();
    let mut workspace_names: Vec<&str> =
        workspace_deps.iter().map(|dep| dep.0.name.as_str()).collect();
    workspace_names.sort_unstable();
    assert_eq!(
        workspace_names,
        ["a", "b"],
        "the root border no longer applies inside the workspace, while its own b border does",
    );
    let dep_b = Rc::clone(
        &workspace_deps.iter().find(|dep| dep.0.name == "b").expect("b hoists within workspace").0,
    );
    let b_deps = dep_b.dependencies.borrow();
    let b_names: Vec<&str> = b_deps.iter().map(|dep| dep.0.name.as_str()).collect();
    assert_eq!(b_names, ["c"], "the workspace's b border keeps c below b");
}

/// A conflict-nested node shared by several parents must keep its own
/// conflicting dependencies reachable from *every* parent. The nested
/// hoist pass must not steal a child out of the shared node into one
/// parent's subtree: the other parents' materialized copies would then
/// resolve the name to the root's (wrong-version) slot.
///
/// Regression shape (from `@teambit/bit`'s lockfile): root holds `b@1`
/// and `d@1`; `c1` and `c2` share one `b@2` node whose dependency `d@2`
/// conflicts with the root's `d@1`.
#[test]
fn nested_hoist_keeps_conflicting_dep_reachable_from_every_parent_of_a_shared_node() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("b"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("d"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c1"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c2"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    for parent in ["c1", "c2"] {
        let mut parent_deps = HashMap::new();
        parent_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
        snapshots.insert(
            dep_key(parent, "1.0.0"),
            SnapshotEntry { dependencies: Some(parent_deps), ..SnapshotEntry::default() },
        );
    }
    let mut b_two_deps = HashMap::new();
    b_two_deps.insert(pkg_name("d"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("b", "2.0.0"),
        SnapshotEntry { dependencies: Some(b_two_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("d", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("d", "2.0.0"), SnapshotEntry::default());

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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["b", "c1", "c2", "d"], "root keeps its direct b@1 and d@1");

    let mut b_under_parents: Vec<Rc<HoisterResult>> = Vec::new();
    for parent_name in ["c1", "c2"] {
        let parent =
            Rc::clone(&root_children.iter().find(|dep| dep.0.name == parent_name).unwrap().0);
        let parent_kids = parent.dependencies.borrow();
        let nested_b = Rc::clone(
            &parent_kids
                .iter()
                .find(|dep| dep.0.name == "b")
                .unwrap_or_else(|| panic!("{parent_name} keeps its conflicting b@2: {parent:#?}"))
                .0,
        );
        assert!(nested_b.references.borrow().contains("b@2.0.0"));

        // `d@2` must resolve from this parent's copy of `b@2`: either
        // nested under `b@2` itself or as a sibling inside the parent.
        let d_under_b = nested_b
            .dependencies
            .borrow()
            .iter()
            .any(|dep| dep.0.name == "d" && dep.0.references.borrow().contains("d@2.0.0"));
        let d_under_parent = parent_kids
            .iter()
            .any(|dep| dep.0.name == "d" && dep.0.references.borrow().contains("d@2.0.0"));
        assert!(
            d_under_b || d_under_parent,
            "d@2 is unreachable from {parent_name}'s subtree, so its b@2 would \
             resolve d to the root's d@1: {parent:#?}",
        );
        b_under_parents.push(nested_b);
    }
    // The copies are per-path (decoupled), never one shared
    // allocation — a shared node would let one parent's hoisting
    // steal the other's subtree.
    assert!(
        !Rc::ptr_eq(&b_under_parents[0], &b_under_parents[1]),
        "each parent gets its own decoupled b@2 copy",
    );
}

/// Peer-suffix variants of one package version collapse onto the
/// first-seen variant (the ported `depPathByPkgId` mapping): the
/// hoister sees a single locator, so the variants dedup at the root
/// instead of conflict-nesting a copy under every dependent. Keeping
/// them distinct made peer-variant-heavy lockfiles (e.g.
/// `@teambit/bit`'s) explode the per-path walk.
#[test]
fn peer_suffix_variants_collapse_to_one_hoisted_copy() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("c1"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c2"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut c1_deps = HashMap::new();
    c1_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("1.0.0(p@1.0.0)")));
    snapshots.insert(
        dep_key("c1", "1.0.0"),
        SnapshotEntry { dependencies: Some(c1_deps), ..SnapshotEntry::default() },
    );
    let mut c2_deps = HashMap::new();
    c2_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("1.0.0(p@2.0.0)")));
    snapshots.insert(
        dep_key("c2", "1.0.0"),
        SnapshotEntry { dependencies: Some(c2_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("x", "1.0.0(p@1.0.0)"), SnapshotEntry::default());
    snapshots.insert(dep_key("x", "1.0.0(p@2.0.0)"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("variant hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["c1", "c2", "x"], "one hoisted x, no nested variant copies");
    for parent in ["c1", "c2"] {
        let parent = &root_children.iter().find(|dep| dep.0.name == parent).unwrap().0;
        assert!(
            parent.dependencies.borrow().is_empty(),
            "the variant edge dedups against the root copy: {parent:#?}",
        );
    }
    let hoisted_x = &root_children.iter().find(|dep| dep.0.name == "x").unwrap().0;
    assert!(
        hoisted_x.references.borrow().contains("x@1.0.0(p@1.0.0)"),
        "the first-seen variant is the canonical reference: {hoisted_x:#?}",
    );
}
