use super::{
    HashMap, HoistOpts, HoisterResult, Lockfile, ProjectSnapshot, Rc, ResolvedDependencyMap,
    SnapshotDepRef, SnapshotEntry, VecDeque, assert_eq, dep_key, empty_lockfile, hoist,
    is_preferred_ident, lockfile_version, pkg_name, resolved_dep, result_node, ver_peer,
};

#[test]
fn deep_chain_flattens_in_one_pass() {
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
    let mut b_deps = HashMap::new();
    b_deps.insert(pkg_name("c"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    let mut c_deps = HashMap::new();
    c_deps.insert(pkg_name("d"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("b", "1.0.0"),
        SnapshotEntry { dependencies: Some(b_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("c", "1.0.0"),
        SnapshotEntry { dependencies: Some(c_deps), ..SnapshotEntry::default() },
    );
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

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b", "c", "d"], "depth-4 chain flattens: {result:#?}");
    for entry in root_children.iter() {
        assert!(entry.0.dependencies.borrow().is_empty(), "{} has no nested deps", entry.0.name);
    }
}

#[test]
fn transitive_npm_alias_resolves_target_snapshot() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("host"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut host_deps = HashMap::new();
    host_deps.insert(pkg_name("aliased-name"), SnapshotDepRef::Alias(dep_key("real-pkg", "2.0.0")));
    snapshots.insert(
        dep_key("host", "1.0.0"),
        SnapshotEntry { dependencies: Some(host_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("real-pkg", "2.0.0"), SnapshotEntry::default());

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

    let result =
        hoist(&lockfile, &HoistOpts::default()).expect("aliased transitive should resolve");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["aliased-name", "host"]);
    let aliased = Rc::clone(
        &root_children
            .iter()
            .find(|dep| dep.0.name == "aliased-name")
            .expect("aliased-name hoisted")
            .0,
    );
    assert_eq!(aliased.name, "aliased-name");
    assert_eq!(aliased.ident_name, "real-pkg");
    let refs = aliased.references.borrow();
    assert!(
        refs.contains("real-pkg@2.0.0"),
        "reference is the resolved snapshot key, not the alias: {refs:?}",
    );
    assert_eq!(refs.len(), 1);
    let host = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "host").unwrap().0);
    assert!(host.dependencies.borrow().is_empty(), "host stripped of its aliased dep: {host:#?}");
}

/// A cycle inside a conflict-nested shared cluster must be cut by the
/// locator path guard in `hoist_subtree` — a cycle that survives into
/// the result sends the layout walkers into unbounded recursion.
#[test]
fn conflict_nested_shared_cycle_is_cut() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("x"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("y"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c1"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c2"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    for parent in ["c1", "c2"] {
        let mut parent_deps = HashMap::new();
        parent_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
        snapshots.insert(
            dep_key(parent, "1.0.0"),
            SnapshotEntry { dependencies: Some(parent_deps), ..SnapshotEntry::default() },
        );
    }
    let mut x_two_deps = HashMap::new();
    x_two_deps.insert(pkg_name("y"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("x", "2.0.0"),
        SnapshotEntry { dependencies: Some(x_two_deps), ..SnapshotEntry::default() },
    );
    let mut y_two_deps = HashMap::new();
    y_two_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("y", "2.0.0"),
        SnapshotEntry { dependencies: Some(y_two_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("x", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("y", "1.0.0"), SnapshotEntry::default());

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

    let result = hoist(&lockfile, &HoistOpts::default()).expect("cyclic hoist should succeed");

    // Walk every path; a node reappearing on its own ancestor path
    // means a cycle survived.
    fn assert_acyclic(node: &Rc<HoisterResult>, path: &mut Vec<*const HoisterResult>) {
        assert!(
            !path.contains(&Rc::as_ptr(node)),
            "cycle in hoister output through {}@{:?}",
            node.name,
            node.references.borrow(),
        );
        path.push(Rc::as_ptr(node));
        let children: Vec<Rc<HoisterResult>> =
            node.dependencies.borrow().iter().map(|dep| Rc::clone(&dep.0)).collect();
        for child in children {
            assert_acyclic(&child, path);
        }
        path.pop();
    }
    let root_children: Vec<Rc<HoisterResult>> =
        result.dependencies.borrow().iter().map(|dep| Rc::clone(&dep.0)).collect();
    let mut path = Vec::new();
    for child in &root_children {
        assert_acyclic(child, &mut path);
    }
}

/// An npm-alias edge that exposes the same underlying package as an
/// ancestor under a *different* alias is not a cycle: it is the only
/// `node_modules/<alias>` entry for its name, so cutting it would
/// break `require('<alias>')`. Only an edge repeating an ancestor's
/// alias *and* locator is cut, mirroring upstream's
/// `aliasedLocatorPath` guard (which compares `name@locator`).
/// Regression shape: `b@1` exposes itself under the alias `c`
/// (`"c": "npm:b@1.0.0"`).
#[test]
fn self_alias_keeps_its_entry_and_only_the_alias_repeat_is_cut() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("b"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut b_deps = HashMap::new();
    b_deps.insert(pkg_name("c"), SnapshotDepRef::Alias(dep_key("b", "1.0.0")));
    snapshots.insert(
        dep_key("b", "1.0.0"),
        SnapshotEntry { dependencies: Some(b_deps), ..SnapshotEntry::default() },
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
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("self-alias hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["b", "c"],
        "the alias keeps its own node_modules entry instead of being cut as a cycle",
    );

    // The alias copy's own self-alias edge repeats both the alias
    // name and the locator, so that one *is* cut — the result must
    // not retain the cycle.
    for child in root_children.iter() {
        assert!(
            child.0.dependencies.borrow().is_empty(),
            "the alias-repeat back-edge is cut: {child:#?}",
        );
    }
}

/// A nearer ancestor holding a different version of a name blocks
/// both hoisting and same-node dedup for that name (upstream's
/// "filled by parent" scan): removing the edge would make this
/// position resolve the ancestor's conflicting copy.
#[test]
fn ancestor_conflict_blocks_dedup_against_the_root_copy() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("x"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("b"), resolved_dep("2.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    let mut b_one_deps = HashMap::new();
    b_one_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("b", "1.0.0"),
        SnapshotEntry { dependencies: Some(b_one_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "2.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("x", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("x", "2.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let node_a = &root_children.iter().find(|dep| dep.0.name == "a").unwrap().0;
    let a_kids = node_a.dependencies.borrow();
    let b_nested = &a_kids.iter().find(|dep| dep.0.name == "b").expect("b@1 nests under a").0;
    let b_kids = b_nested.dependencies.borrow();
    let x_kept = b_kids
        .iter()
        .find(|dep| dep.0.name == "x")
        .unwrap_or_else(|| panic!("b@1 keeps its own x@1 below a's conflicting x@2: {node_a:#?}"));
    assert!(x_kept.0.references.borrow().contains("x@1.0.0"));
}

/// A nested hoist root must not take a name slot that its subtree
/// already resolves through an ancestor directory (upstream's
/// `usedDependencies` gate). Regression shape (from `express` in
/// `@teambit/bit`'s lockfile): `send`'s `ms@2` dedup'd against the
/// root, then the nested pass hoisted `debug`'s conflicting `ms@1`
/// onto `express`, shadowing `send`'s resolution.
#[test]
fn nested_root_does_not_shadow_names_its_subtree_uses_from_above() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("e"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("m"), resolved_dep("2.0.0"));
    root_deps.insert(pkg_name("s"), resolved_dep("2.0.0"));
    root_deps.insert(pkg_name("d"), resolved_dep("2.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut e_deps = HashMap::new();
    e_deps.insert(pkg_name("d"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    e_deps.insert(pkg_name("s"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("e", "1.0.0"),
        SnapshotEntry { dependencies: Some(e_deps), ..SnapshotEntry::default() },
    );
    let mut d_one_deps = HashMap::new();
    d_one_deps.insert(pkg_name("m"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("d", "1.0.0"),
        SnapshotEntry { dependencies: Some(d_one_deps), ..SnapshotEntry::default() },
    );
    let mut s_one_deps = HashMap::new();
    s_one_deps.insert(pkg_name("m"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("s", "1.0.0"),
        SnapshotEntry { dependencies: Some(s_one_deps), ..SnapshotEntry::default() },
    );
    for leaf in [("m", "1.0.0"), ("m", "2.0.0"), ("s", "2.0.0"), ("d", "2.0.0")] {
        snapshots.insert(dep_key(leaf.0, leaf.1), SnapshotEntry::default());
    }

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let node_e = &root_children.iter().find(|dep| dep.0.name == "e").unwrap().0;
    let e_kids = node_e.dependencies.borrow();
    assert!(
        !e_kids.iter().any(|dep| dep.0.name == "m"),
        "m@1 must not hoist onto e - s@1's requires resolve m through the root: {node_e:#?}",
    );
    let d_nested = &e_kids.iter().find(|dep| dep.0.name == "d").expect("d@1 nests under e").0;
    let m_kept = d_nested
        .dependencies
        .borrow()
        .iter()
        .any(|dep| dep.0.name == "m" && dep.0.references.borrow().contains("m@1.0.0"));
    assert!(m_kept, "d@1 keeps its own m@1 nested: {d_nested:#?}");
}

/// `is_preferred_ident` returns `true` for a name with no entry in the
/// ident map. Unreachable from `hoist` (the map covers every name the
/// walk encounters except root peers, and `hoist` builds the `.` root
/// with no peers), so it is exercised by calling the guard directly.
#[test]
fn is_preferred_ident_allows_names_absent_from_the_map() {
    let child = result_node("ghost", "ghost@1.0.0", &[], vec![]);
    let ident_map: HashMap<String, VecDeque<String>> = HashMap::new();
    assert!(
        is_preferred_ident(&child, &ident_map),
        "a name with no preference entry carries no constraint and hoists freely",
    );
}

/// `is_preferred_ident` returns `true` when a name maps to an empty
/// candidate list. [`build_hoist_ident_map`] never emits an empty
/// `VecDeque` (every entry gets at least one ident), so this defensive
/// guard is only reachable by constructing the empty list directly.
#[test]
fn is_preferred_ident_allows_empty_candidate_lists() {
    let child = result_node("ghost", "ghost@1.0.0", &[], vec![]);
    let ident_map: HashMap<String, VecDeque<String>> =
        HashMap::from([("ghost".to_string(), VecDeque::new())]);
    assert!(
        is_preferred_ident(&child, &ident_map),
        "an empty candidate list carries no constraint and hoists freely",
    );
}
