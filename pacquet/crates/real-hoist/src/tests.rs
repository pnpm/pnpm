use super::{HoistError, HoistOpts, HoisterResult, hoist};
use pacquet_lockfile::{
    ComVer, Lockfile, LockfileSettings, LockfileVersion, PkgName, PkgNameVerPeer, PkgVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeSet, HashMap},
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

/// Direct port of the upstream "hoist throws an error if the
/// lockfile is broken" test at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/test/index.ts>.
/// The root importer references `foo@1.0.0` but the `snapshots`
/// map is empty, so the wrapper's snapshot lookup must surface
/// `LockfileMissingDependency` rather than silently produce a
/// truncated tree.
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

/// An empty lockfile (no importers at all) hoists to an empty
/// result. Sanity-checks the wrapper's "no root importer" branch
/// and the stub `nm_hoist` end-to-end.
#[test]
fn empty_lockfile_yields_empty_root() {
    let lockfile = empty_lockfile();
    let result = hoist(&lockfile, &HoistOpts::default()).expect("empty hoist should succeed");
    assert_eq!(result.name, ".");
    assert_eq!(result.ident_name, ".");
    assert!(result.dependencies.borrow().is_empty(), "no importers means no children at the root");
}

/// `root → a → b` collapses to `root → {a, b}` because `b` has no
/// name conflict at root. Pins the simplest hoisting case: a
/// single transitive dep surfaces at the root and its old parent
/// no longer carries it.
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

/// Diamond dependency `root → {a, c}` with both `a → b@1` and
/// `c → b@1` (same `Rc` thanks to the wrapper's identity dedup).
/// After hoist, `b` appears once at root and its old parents
/// `a` and `c` carry no transitive deps.
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

    // Both a@1 and c@1 depend on b@1 — same dep_key → same Rc in
    // the input HoisterTree, same Rc in the result graph.
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

    // Walk the whole result graph and collect every distinct
    // allocation whose `name == "b"`. The wrapper deduped a@1's b
    // and c@1's b into one `Rc<HoisterResult>` (the diamond shares
    // by identity), and the hoist must preserve that identity
    // rather than allocating a second copy somewhere — so the set
    // of pointers we collect has exactly one entry.
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

/// Version conflict: `root → {a, c}` with `a → b@1` and
/// `c → b@2`. The first DFS reach wins root's `b` slot; the
/// other version stays under its declaring parent.
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

    // a@1 → b@1, c@1 → b@2.
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
    // DFS visits root's direct deps in alias order (`a` then
    // `c`), so `a@1`'s `b@1.0.0` reaches root first and wins the
    // slot. Assert membership (not iteration-order-derived
    // equality) so the test stays focused on which reference is
    // present, not on which one happens to come back first from
    // the set.
    let b_refs = b_at_root.references.borrow();
    assert!(b_refs.contains("b@1.0.0"), "first DFS visitor wins root slot: {b_refs:?}");
    assert_eq!(b_refs.len(), 1, "no other reference accumulated yet: {b_refs:?}");
    // `c`'s `b@2` remains under `c`.
    let dep_c = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "c").unwrap().0);
    let c_kids = dep_c.dependencies.borrow();
    assert_eq!(c_kids.len(), 1, "c kept its conflicting b@2");
    let b_under_c_refs = c_kids[0].0.references.borrow();
    assert!(b_under_c_refs.contains("b@2.0.0"), "loser stays under c: {b_under_c_refs:?}");
    assert_eq!(b_under_c_refs.len(), 1);
}

/// Deep linear chain `root → a → b → c → d` flattens to
/// `root → {a, b, c, d}` in a single hoist round: each node, by
/// the time DFS descends into it, has already been moved up to
/// root, so its own children evaluate against root's slots
/// (which are all free).
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

/// `external_dependencies` are added as `link:` placeholders at the
/// root so the inner hoister won't hoist anything else into those
/// name slots, and they're stripped from the result after hoisting.
/// Pin both: the placeholder doesn't leak into the result, and any
/// real package the lockfile contributes still does.
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

/// A transitive npm-alias dep (`SnapshotDepRef::Alias`) must look
/// up the snapshot under the *target* package name, not under the
/// alias. Regression for the wrapper's earlier bug where the
/// snapshot key was reconstructed from `(alias, suffix)` instead
/// of the resolved key — that produced
/// `LockfileMissingDependency` on real npm-aliased transitives.
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
    // `host@1.0.0` depends on `aliased-name` resolved to the
    // target snapshot `real-pkg@2.0.0` — i.e. an npm-alias.
    let mut host_deps = HashMap::new();
    host_deps.insert(pkg_name("aliased-name"), SnapshotDepRef::Alias(dep_key("real-pkg", "2.0.0")));
    snapshots.insert(
        dep_key("host", "1.0.0"),
        SnapshotEntry { dependencies: Some(host_deps), ..SnapshotEntry::default() },
    );
    // Snapshot lookup must target `real-pkg@2.0.0`, NOT
    // `aliased-name@2.0.0`. If we put only the target key in the
    // map, the wrapper succeeds; if it builds the key from the
    // alias the lookup misses and we get
    // `LockfileMissingDependency`.
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
    // After hoist, both `host` and `aliased-name` sit at root —
    // `aliased-name` had no conflict so it floats up. The npm-
    // alias indirection is observable on the hoisted node itself:
    // `name` is the exposed alias, `ident_name` and `references`
    // carry the resolved target's identity.
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

/// Peer-shadow refusal: `app → widget (peer: react) + widget → react@17`
/// and `root → react@18`. `widget` declares `react` as a peer, its
/// only ancestor (`app`) supplies `react@17`, and the root carries
/// `react@18`. Hoisting `widget` to root would silently re-resolve
/// its peer to react@18 instead of the ancestor-supplied react@17,
/// so the algorithm leaves `widget` nested under `app`.
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

    // `app@1.0.0` brings in both `widget` and `react@17`.
    // `widget@1.0.0` declares `react` as a peer dependency. The
    // snapshot graph itself doesn't list `react` under `widget`
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
    // Root has the two direct deps; `widget` is NOT at root.
    assert_eq!(names, ["app", "react"], "widget stays under app: {result:#?}");
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    let app_kids = app.dependencies.borrow();
    let app_names: Vec<&str> = app_kids.iter().map(|dep| dep.0.name.as_str()).collect();
    assert!(
        app_names.contains(&"widget"),
        "widget nested under app to keep ancestor peer resolution: {app_names:?}",
    );
    // The conflicting react@17 also stays nested under app
    // (parent-wins kicked in because root already has react@18).
    assert!(app_names.contains(&"react"), "app keeps its own react@17: {app_names:?}");
}

/// Regression for the stale-ancestor-path bug: a peer-constrained
/// leaf whose intermediate parent hoists to the root must be
/// evaluated against the parent's *post-hoist* ancestor chain, not
/// against the (now-irrelevant) original chain. The previous BFS
/// captured the path at queue time, so when an intermediate node
/// got hoisted between being queued and dequeued, the leaf would
/// be checked against ex-ancestors and over-refused.
///
/// Setup: `root → {app, react@18}`, `app → {react@17, mid}`,
/// `mid → terminal (peer: react)`. After hoist:
/// - `app` and `react@18` are direct deps of root.
/// - `react@17` stays under `app` because root's `react` slot is
///   already taken by `react@18` (parent-wins).
/// - `mid` has no name conflict at root, so it hoists.
/// - `terminal` has peer `react`. With the *post-hoist* path
///   `[root, mid]`, neither `mid` nor `root` provides a peer
///   ident that disagrees with what `root` carries
///   (`root.react@18` is consistent with itself), so `terminal`
///   hoists too. The previous BFS would have used the stale path
///   `[root, app, mid]`, seen `app.react@17 ≠ root.react@18`, and
///   refused — leaving terminal nested under `mid` for no real
///   reason.
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
    // The whole chain flattens: mid (no name conflict) hoists to
    // root, and terminal (peer-friendly along its post-hoist
    // path) hoists past mid.
    assert_eq!(
        names,
        ["app", "mid", "react", "terminal"],
        "mid and terminal hoist freely: {result:#?}",
    );
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    let app_deps = app.dependencies.borrow();
    let app_names: Vec<&str> = app_deps.iter().map(|dep| dep.0.name.as_str()).collect();
    // app keeps its conflicting react@17 (parent-wins), but mid
    // has moved to root so app no longer carries it.
    assert_eq!(app_names, ["react"], "app retains conflicting react@17: {app_names:?}");
    drop(app_deps);
    let mid = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "mid").unwrap().0);
    assert!(mid.dependencies.borrow().is_empty(), "mid stripped of terminal: {mid:#?}");
}

/// Peer-friendly hoist: `app → widget (peer: react)`, `app → react@18`,
/// `root → react@18`. The peer name `react` is provided by both
/// `app` and the root with the *same* ident (`react@18`, shared `Rc`
/// thanks to the wrapper's identity dedup), so hoisting `widget` to
/// root doesn't change its peer resolution — the algorithm allows
/// the hoist.
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
    // widget hoists to root because the peer resolves identically
    // at root and at app.
    assert_eq!(names, ["app", "react", "widget"], "widget hoists past app: {result:#?}");
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    assert!(
        app.dependencies.borrow().is_empty(),
        "app stripped of its hoisted widget + dedup'd react: {app:#?}",
    );
}

/// Multi-round convergence: a peer-constrained candidate that
/// gets refused in round 1 because a sibling provides the peer
/// with no matching root slot. Once round 1 hoists the sibling
/// out, round 2 reconsiders the candidate against the new state
/// and lets it through.
///
/// Setup: `root → app → {widget (peer: x), x@1}`. Root has no
/// `x` of its own. Iteration order over `app`'s children is
/// alphabetical, so `widget` is visited *before* `x` in round 1:
///
/// - Round 1: `widget`'s peer check sees `app.x@1` and root with
///   no `x` → mismatch → `PeerShadow`, leave at `app`. Then `x`
///   gets evaluated: free at root → hoist. End of round 1:
///   `root.deps = {app, x@1}`, `app.deps = {widget}`.
/// - Round 2: walk again. `widget`'s peer check now sees `app`
///   without `x` (it moved out in round 1) and root with `x@1`.
///   No ancestor disagrees with root's slot → no shadow →
///   `Free` → hoist. End of round 2: `root.deps = {app, x@1,
///   widget}`, `app.deps = {}`.
/// - Round 3: no moves, loop terminates.
///
/// The previous single-pass DFS would have left `widget` nested
/// forever; multi-round converges to the correct flat layout.
#[test]
fn multi_round_unlocks_peer_friendly_hoist_after_blocker_moves() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("app"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    // app@1 depends on widget@1 and x@1. widget@1 declares x as a
    // peer (via the `packages:` map). Root carries no x of its
    // own, so the only x in scope before hoisting is the one
    // under app — exactly the multi-round trigger.
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
    // All three at root after multi-round convergence.
    assert_eq!(
        names,
        ["app", "widget", "x"],
        "widget hoists in round 2 after x clears app in round 1: {result:#?}",
    );
    let app = Rc::clone(&root_children.iter().find(|dep| dep.0.name == "app").unwrap().0);
    assert!(app.dependencies.borrow().is_empty(), "app stripped after multi-round: {app:#?}");
}

/// A `hoisting_limits` border keeps a bordered node's descendants
/// nested. Ports the spirit of upstream's `should not hoist packages
/// past hoist boundary`. Setup: `root → a → b`. With no limits, `b`
/// would flatten to root (see `one_transitive_dep_hoists_to_root`).
/// With `hoisting_limits[".@"] = {a}`, `a` is a border, so its
/// descendant `b` stays nested under `a`. The border node `a` itself
/// still sits at root (a border blocks a node's children, not the
/// node).
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

/// A border keeps *every* descendant of the bordered node nested,
/// not just the first. Ports the spirit of upstream's `should not
/// hoist multiple package past nohoist root`. Setup: `root → a →
/// {b, c, d}` with `hoisting_limits[".@"] = {a}`. All three of a's
/// deps stay under a.
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
    // Only the border node `a` sits at root; all of its deps stay
    // nested beneath it.
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

/// `hoisting_limits` keyed on a different importer (one we don't
/// hoist into) is silently ignored. The wrapper passes the whole
/// map through, and the algorithm only consults entries matching
/// the current root locator. Sanity test for non-interference.
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
    // b still hoists because the limits don't apply to `.@`.
    assert_eq!(names, ["a", "b"], "limits keyed elsewhere don't affect root hoist: {result:#?}");
}

/// Self-dependency: a package that lists itself as a transitive
/// dep. Upstream tolerates this (see `should tolerate
/// self-dependencies` in `@yarnpkg/nm/tests/hoist.test.ts`).
/// The wrapper's dedup-by-cache keeps a single `Rc` for `a@1`,
/// and the hoist sees the back-edge to itself as a cycle that
/// the DFS skips via its `visited` set. No infinite loop, sane
/// output.
#[test]
fn self_dependency_does_not_loop() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    // a@1 depends on itself.
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
    // The self-edge is dedup'd by the wrapper's identity cache to
    // the same Rc as the root's `a`. During hoist, the back-edge
    // to root is skipped; the self-edge under a is dedup'd as
    // SameNode (a is at root via the same Rc) and stripped.
    assert!(dep_a.dependencies.borrow().is_empty(), "self-edge stripped: {dep_a:#?}");
}

/// Basic two-node cycle: `a → b → a`. Both packages share the
/// `Rc` for `a` and `b` thanks to the wrapper's dedup, so the
/// hoist back-edge is skipped and the algorithm terminates.
/// Ports the spirit of upstream's `should support basic cyclic
/// dependencies`.
#[test]
fn basic_cyclic_dependency_terminates() {
    let mut importers = HashMap::new();
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("a"), resolved_dep("1.0.0"));
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(root_deps), ..ProjectSnapshot::default() },
    );

    // a → b → a (cycle).
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

/// A lockfile with importers beyond `.` (a workspace) is now
/// accepted: each non-root importer becomes a `Workspace`-kind
/// child of the virtual `.` root. Mirrors upstream's
/// [`installing/linking/real-hoist/src/index.ts:51-66`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L51-L66)
/// where `hoistWorkspacePackages` (default true) drives the same
/// transformation. The walker (slice 4) and linker (slice 5) then
/// fan out per-importer.
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

/// `hoist_workspace_packages: false` opts out of including non-root
/// importers in the shared tree. The hoister output then carries
/// only the root importer's deps (empty in this fixture). Pacquet
/// exposes this via [`pacquet_config::Config::hoist_workspace_packages`].
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
