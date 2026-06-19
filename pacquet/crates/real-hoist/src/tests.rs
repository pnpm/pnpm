use super::{
    HoistError, HoistOpts, HoisterResult, RcByPtr, build_hoist_ident_map, hoist, is_preferred_ident,
};
use indexmap::IndexSet;
use pacquet_lockfile::{
    ComVer, Lockfile, LockfileSettings, LockfileVersion, PkgName, PkgNameVerPeer, PkgVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, VecDeque},
    rc::Rc,
};

fn lockfile_version() -> LockfileVersion<9> {
    LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfileVersion 9.0 is compatible")
}

fn pkg_name(name: &str) -> PkgName {
    PkgName::parse(name).expect("parse PkgName")
}

fn ver_peer(spec: &str) -> PkgVerPeer {
    spec.parse::<PkgVerPeer>().expect("parse PkgVerPeer")
}

fn dep_key(name: &str, version: &str) -> PkgNameVerPeer {
    PkgNameVerPeer::new(pkg_name(name), ver_peer(version))
}

fn resolved_dep(version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec { specifier: version.to_string(), version: ver_peer(version).into() }
}

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
    }
}

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
    };

    let err = hoist(&lockfile, &HoistOpts::default()).expect_err("missing snapshot should error");
    let HoistError::LockfileMissingDependency { pkg_key } = err;
    assert_eq!(pkg_key, "foo@1.0.0");
}

#[test]
fn empty_lockfile_yields_empty_root() {
    let lockfile = empty_lockfile();
    let result = hoist(&lockfile, &HoistOpts::default()).expect("empty hoist should succeed");
    assert_eq!(result.name, ".");
    assert_eq!(result.ident_name, ".");
    assert!(result.dependencies.borrow().is_empty(), "no importers means no children at the root");
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
fn version_conflict_keeps_loser_at_parent() {
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
    snapshots.insert(dep_key("b", "2.0.0"), SnapshotEntry::default());

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
    };

    let result = hoist(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let root_children = result.dependencies.borrow();
    let mut names: Vec<&str> = root_children.iter().map(|dep| dep.0.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b", "c"], "root has a, c, and one b");
    let b_at_root = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "b").unwrap().0);
    let b_refs = b_at_root.references.borrow();
    assert!(b_refs.contains("b@1.0.0"), "first DFS visitor wins root slot: {b_refs:?}");
    assert_eq!(b_refs.len(), 1, "no other reference accumulated yet: {b_refs:?}");
    let dep_c = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "c").unwrap().0);
    let c_kids = dep_c.dependencies.borrow();
    assert_eq!(c_kids.len(), 1, "c kept its conflicting b@2");
    let b_under_c_refs = c_kids[0].0.references.borrow();
    assert!(b_under_c_refs.contains("b@2.0.0"), "loser stays under c: {b_under_c_refs:?}");
    assert_eq!(b_under_c_refs.len(), 1);
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

/// Helper for the peer-aware tests: build a `PackageMetadata`
/// whose `packages:`-level `peer_dependencies` claims one peer.
fn pkg_metadata_with_peer(peer_name: &str) -> pacquet_lockfile::PackageMetadata {
    use pacquet_lockfile::{LockfileResolution, PackageMetadata, TarballResolution};
    let mut peer_deps = HashMap::new();
    peer_deps.insert(peer_name.to_string(), "*".to_string());
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://example.invalid/{peer_name}-host.tgz"),
            integrity: None,
            git_hosted: None,
            path: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: Some(peer_deps),
        peer_dependencies_meta: None,
    }
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

#[test]
fn hoist_workspace_packages_false_omits_workspace_children() {
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
    };

    let opts = HoistOpts { hoist_workspace_packages: false, ..HoistOpts::default() };
    let result = hoist(&lockfile, &opts).expect("hoist succeeds");
    assert!(
        result.dependencies.borrow().is_empty(),
        "non-root importers omitted: {:?}",
        result.dependencies.borrow().iter().map(|child| child.0.name.clone()).collect::<Vec<_>>(),
    );
}

/// Construct a [`HoisterResult`] node directly, for unit-testing the
/// preference machinery without going through [`hoist`] (which only ever
/// builds the `.` root with empty `peer_names`).
fn result_node(
    name: &str,
    reference: &str,
    peer_names: &[&str],
    dependencies: Vec<Rc<HoisterResult>>,
) -> Rc<HoisterResult> {
    Rc::new(HoisterResult {
        name: name.to_string(),
        ident_name: name.to_string(),
        references: RefCell::new(BTreeSet::from([reference.to_string()])),
        peer_names: peer_names.iter().map(|&peer| peer.to_string()).collect(),
        dependencies: RefCell::new(dependencies.into_iter().map(RcByPtr).collect::<IndexSet<_>>()),
    })
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
