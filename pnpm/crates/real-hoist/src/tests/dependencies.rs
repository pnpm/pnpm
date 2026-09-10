use super::{
    HashMap, HoistOpts, Lockfile, ProjectSnapshot, Rc, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry, assert_eq, dep_key, empty_lockfile,
    hoist, lockfile_version, percent_encode_path, pkg_metadata_with_peer, pkg_name, resolved_dep,
    ver_peer,
};

#[test]
fn version_conflict_keeps_loser_at_parent() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("c"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("d"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    let mut c_deps = HashMap::new();
    c_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("c", "1.0.0"),
        SnapshotEntry { dependencies: Some(c_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("b", "1.0.0"), SnapshotEntry::default());
    let mut b_two_deps = HashMap::new();
    b_two_deps.insert(pkg_name("d"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("b", "2.0.0"),
        SnapshotEntry { dependencies: Some(b_two_deps), ..SnapshotEntry::default() },
    );
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
    assert_eq!(names, ["a", "b", "c", "d"], "root keeps its direct d and one b");
    let b_at_root = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "b").unwrap().0);
    let b_refs = b_at_root.references.borrow();
    assert!(b_refs.contains("b@1.0.0"), "first DFS visitor wins root slot: {b_refs:?}");
    assert_eq!(b_refs.len(), 1, "no other reference accumulated yet: {b_refs:?}");
    let dep_c = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "c").unwrap().0);
    let c_kids = dep_c.dependencies.borrow();
    assert_eq!(c_kids.len(), 2, "c kept b@2 and hoisted its conflicting d@2");
    let b_under_c = Rc::clone(&c_kids.iter().find(|dep| dep.0.name == "b").unwrap().0);
    let b_under_c_refs = b_under_c.references.borrow();
    assert!(b_under_c_refs.contains("b@2.0.0"), "loser stays under c: {b_under_c_refs:?}");
    assert_eq!(b_under_c_refs.len(), 1);
    assert!(b_under_c.dependencies.borrow().is_empty(), "b's descendants hoist to nested root c");
    let d_under_c_refs = c_kids.iter().find(|dep| dep.0.name == "d").unwrap().0.references.borrow();
    assert!(d_under_c_refs.contains("d@2.0.0"), "d@2 stays below the root's d@1 conflict");
}

/// The most-depended-on version of a shared name wins the root
/// slot, even when a less-used version is discovered first in the
/// depth-first walk — a first-visitor rule would hoist the wrong
/// one. Ports the "most used version wins" guarantee of yarn's
/// `getHoistIdentMap`.
#[test]
fn most_used_version_wins_root_slot() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("aa"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("cc"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("dd"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut aa_deps = HashMap::new();
    aa_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    let mut cc_deps = HashMap::new();
    cc_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    let mut dd_deps = HashMap::new();
    dd_deps.insert(pkg_name("x"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    snapshots.insert(
        dep_key("aa", "1.0.0"),
        SnapshotEntry { dependencies: Some(aa_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("cc", "1.0.0"),
        SnapshotEntry { dependencies: Some(cc_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("dd", "1.0.0"),
        SnapshotEntry { dependencies: Some(dd_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("x", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("x", "2.0.0"), SnapshotEntry::default());

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
    assert_eq!(names, ["aa", "cc", "dd", "x"], "one x at root: {result:#?}");
    let x_at_root = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "x").unwrap().0);
    let x_refs = x_at_root.references.borrow();
    assert!(
        x_refs.contains("x@2.0.0"),
        "the more-used x@2.0.0 wins root over the first-visited x@1.0.0: {x_refs:?}",
    );
    let dep_aa = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "aa").unwrap().0);
    let aa_kids = dep_aa.dependencies.borrow();
    assert_eq!(aa_kids.len(), 1, "aa keeps its conflicting x@1.0.0");
    let x_under_aa = aa_kids[0].0.references.borrow();
    assert!(
        x_under_aa.contains("x@1.0.0"),
        "the less-used x stays nested under aa: {x_under_aa:?}",
    );
}

#[test]
fn external_dependencies_are_stripped_from_the_result() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("real"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(dep_key("real", "1.0.0"), SnapshotEntry::default());

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

    let opts = HoistOpts {
        external_dependencies: std::iter::once("bit-managed".to_string()).collect(),
        ..HoistOpts::default()
    };
    let result = hoist(&lockfile, &opts).expect("hoist should succeed");
    let names: Vec<String> =
        result.dependencies.borrow().iter().map(|dep| dep.name.clone()).collect();
    assert_eq!(names, ["real"], "external dep is stripped, real dep remains: {names:?}");
}

#[test]
fn peer_constrained_node_stays_under_parent_when_root_provides_different_ident() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("app"), resolved_dep("1.0.0"));
    root_deps.insert(pkg_name("react"), resolved_dep("18.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    // The snapshot graph itself doesn't list `react` under `widget`
    // (peers aren't snapshot edges), so `widget`'s ancestor for
    // peer resolution is `app`.
    let mut snapshots = HashMap::new();
    let mut app_deps = HashMap::new();
    app_deps.insert(pkg_name("widget"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    app_deps.insert(pkg_name("react"), SnapshotDepRef::Plain(ver_peer("17.0.0")));
    snapshots.insert(
        dep_key("app", "1.0.0"),
        SnapshotEntry { dependencies: Some(app_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("widget", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(dep_key("react", "17.0.0"), SnapshotEntry::default());
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
    assert_eq!(names, ["app", "react"], "widget stays under app: {result:#?}");
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    let app_kids = app.dependencies.borrow();
    let app_names: Vec<&str> = app_kids.iter().map(|dep| dep.0.name.as_str()).collect();
    assert!(
        app_names.contains(&"widget"),
        "widget nested under app to keep ancestor peer resolution: {app_names:?}",
    );
    assert!(app_names.contains(&"react"), "app keeps its own react@17: {app_names:?}");
}

/// Peer variants of an injected directory dependency are exempt from
/// the collapse — see [`pkg_id`] for why. The fixture mirrors the
/// teambit/bit root-components layout that caught the regression: two
/// importers on the same `file:` package, pinning conflicting peers.
#[test]
fn file_dep_peer_variants_keep_their_own_copies() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), ProjectSnapshot::default());
    for (importer_id, peer_ver) in
        [("node_modules/.bit_roots/r1", "1.0.0"), ("node_modules/.bit_roots/r2", "2.0.0")]
    {
        let mut deps = ResolvedDependencyMap::new();
        deps.insert(
            pkg_name("comp"),
            ResolvedDependencySpec {
                specifier: "workspace:*".to_string(),
                version: ver_peer(&format!("file:comp(p@{peer_ver})")).into(),
            },
        );
        deps.insert(pkg_name("p"), resolved_dep(peer_ver));
        importers.insert(
            importer_id.to_string(),
            ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
        );
    }

    let mut snapshots = HashMap::new();
    for peer_ver in ["1.0.0", "2.0.0"] {
        let mut comp_deps = HashMap::new();
        comp_deps.insert(pkg_name("p"), SnapshotDepRef::Plain(ver_peer(peer_ver)));
        snapshots.insert(
            dep_key("comp", &format!("file:comp(p@{peer_ver})")),
            SnapshotEntry { dependencies: Some(comp_deps), ..SnapshotEntry::default() },
        );
        snapshots.insert(dep_key("p", peer_ver), SnapshotEntry::default());
    }

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let result =
        hoist(&lockfile, &HoistOpts::default()).expect("file-variant hoist should succeed");

    // Node resolution walks up from the importer, so the copy an
    // importer sees is the one nested in its own subtree — or the
    // root's, when its variant was hoisted there. Either placement is
    // correct only if the reference reached this way is the variant
    // the importer declared.
    let root_children = result.dependencies.borrow();
    let comp_reference_seen_by = |importer: &str| -> String {
        let importer_name = percent_encode_path(importer);
        let importer_node =
            &root_children.iter().find(|dep| dep.0.name == importer_name).unwrap().0;
        let importer_children = importer_node.dependencies.borrow();
        let comp = importer_children
            .iter()
            .find(|dep| dep.0.name == "comp")
            .or_else(|| root_children.iter().find(|dep| dep.0.name == "comp"))
            .unwrap_or_else(|| panic!("no comp reachable from {importer}"));
        comp.0.references.borrow().iter().next().cloned().unwrap_or_default()
    };
    assert_eq!(
        comp_reference_seen_by("node_modules/.bit_roots/r1"),
        "comp@file:comp(p@1.0.0)",
        "r1 must resolve the copy carrying its own peer variant",
    );
    assert_eq!(
        comp_reference_seen_by("node_modules/.bit_roots/r2"),
        "comp@file:comp(p@2.0.0)",
        "r2 must resolve the copy carrying its own peer variant",
    );
}

/// The directory exemption in [`pkg_id`] must not widen to local
/// tarballs: a `file:*.tgz` dependency collapses its peer variants
/// like a registry package.
#[test]
fn file_tarball_peer_variants_collapse_like_registry_packages() {
    let mut importers = HashMap::new();
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), ProjectSnapshot::default());
    for (importer_id, peer_ver) in [("packages/a", "1.0.0"), ("packages/b", "2.0.0")] {
        let mut deps = ResolvedDependencyMap::new();
        deps.insert(
            pkg_name("tarpkg"),
            ResolvedDependencySpec {
                specifier: "file:tarpkg.tgz".to_string(),
                version: ver_peer(&format!("file:tarpkg.tgz(p@{peer_ver})")).into(),
            },
        );
        deps.insert(pkg_name("p"), resolved_dep(peer_ver));
        importers.insert(
            importer_id.to_string(),
            ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
        );
    }

    let mut snapshots = HashMap::new();
    for peer_ver in ["1.0.0", "2.0.0"] {
        let mut tar_deps = HashMap::new();
        tar_deps.insert(pkg_name("p"), SnapshotDepRef::Plain(ver_peer(peer_ver)));
        snapshots.insert(
            dep_key("tarpkg", &format!("file:tarpkg.tgz(p@{peer_ver})")),
            SnapshotEntry { dependencies: Some(tar_deps), ..SnapshotEntry::default() },
        );
        snapshots.insert(dep_key("p", peer_ver), SnapshotEntry::default());
    }

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let result =
        hoist(&lockfile, &HoistOpts::default()).expect("tarball-variant hoist should succeed");
    let root_children = result.dependencies.borrow();
    let tarpkg = &root_children.iter().find(|dep| dep.0.name == "tarpkg").unwrap().0;
    assert!(
        tarpkg.references.borrow().contains("tarpkg@file:tarpkg.tgz(p@1.0.0)"),
        "the first-seen variant is the canonical reference: {tarpkg:#?}",
    );
    for importer in ["packages%2Fa", "packages%2Fb"] {
        let importer_node = &root_children.iter().find(|dep| dep.0.name == importer).unwrap().0;
        assert!(
            !importer_node.dependencies.borrow().iter().any(|dep| dep.0.name == "tarpkg"),
            "a tarball peer variant must dedup against the root copy: {importer_node:#?}",
        );
    }
}

#[test]
fn self_dependency_does_not_loop() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    let mut snapshots = HashMap::new();
    let mut a_deps = HashMap::new();
    a_deps.insert(pkg_name("a"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
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

    let result = hoist(&lockfile, &HoistOpts::default()).expect("self-dep should not loop");
    let root_children = result.dependencies.borrow();
    let names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    assert_eq!(names, ["a"], "single a at root: {result:#?}");
    let dep_a = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "a").unwrap().0);
    assert!(dep_a.dependencies.borrow().is_empty(), "self-edge stripped: {dep_a:#?}");
}

#[test]
fn basic_cyclic_dependency_terminates() {
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
    b_deps.insert(pkg_name("a"), SnapshotDepRef::Plain(ver_peer("1.0.0")));
    snapshots.insert(
        dep_key("a", "1.0.0"),
        SnapshotEntry { dependencies: Some(a_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(
        dep_key("b", "1.0.0"),
        SnapshotEntry { dependencies: Some(b_deps), ..SnapshotEntry::default() },
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
        &HoistOpts::default(),
    )
    .expect("cycle should not loop");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b"], "both a and b flatten to root: {result:#?}");
    let dep_a = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "a").unwrap().0);
    let dep_b = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "b").unwrap().0);
    assert!(dep_a.dependencies.borrow().is_empty(), "a's b hoisted away: {dep_a:#?}");
    assert!(dep_b.dependencies.borrow().is_empty(), "b's back-edge to a stripped: {dep_b:#?}");
}
