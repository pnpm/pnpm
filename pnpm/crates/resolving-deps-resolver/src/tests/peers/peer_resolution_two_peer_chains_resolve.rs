use super::{
    DepPath, DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions,
    ResolvePeersOptions, StubResolver, assert_eq, fake_manifest, fake_result,
    resolve_dependency_tree, resolve_peers,
};

/// Two parallel peer chains in one importer — each peer resolves
/// against its own sibling, no cross-pollination. Stands in for
/// the npm-alias peer case; the real alias case needs `npm:`
/// plumbing in the stub resolver.
// TODO(pacquet#?): replace with the real `npm:foo@2` alias once
// `parse_bare_specifier` routes npm-alias specifiers through the
// stub resolver in tests.
#[tokio::test]
async fn two_peer_chains_resolve_against_their_own_sibling() {
    let mut table = HashMap::default();
    table.insert(
        ("foo-a".to_string(), "1.0.0".to_string()),
        fake_result(
            "foo-a",
            "1.0.0",
            serde_json::json!({
                "name": "foo-a",
                "version": "1.0.0",
                "peerDependencies": { "bar-a": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("foo-b".to_string(), "1.0.0".to_string()),
        fake_result(
            "foo-b",
            "1.0.0",
            serde_json::json!({
                "name": "foo-b",
                "version": "1.0.0",
                "peerDependencies": { "bar-b": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar-a".to_string(), "1.0.0".to_string()),
        fake_result("bar-a", "1.0.0", serde_json::json!({ "name": "bar-a", "version": "1.0.0" })),
    );
    table.insert(
        ("bar-b".to_string(), "1.0.0".to_string()),
        fake_result("bar-b", "1.0.0", serde_json::json!({ "name": "bar-b", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "foo-a": "1.0.0", "bar-a": "1.0.0",
        "foo-b": "1.0.0", "bar-b": "1.0.0",
    }));

    let mut tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert_eq!(
        result.direct_dependencies_by_alias.get("foo-a"),
        Some(&DepPath::from("foo-a@1.0.0(bar-a@1.0.0)".to_string())),
    );
    assert_eq!(
        result.direct_dependencies_by_alias.get("foo-b"),
        Some(&DepPath::from("foo-b@1.0.0(bar-b@1.0.0)".to_string())),
    );
    assert!(result.peer_dependency_issues.missing.is_empty());
}

/// A peer satisfied by a wrong-version sibling inside the
/// parent's subtree surfaces as a *bad* peer (not missing). The
/// `resolvedFrom` field isn't exposed on pacquet's
/// `PeerDependencyIssue` yet.
#[tokio::test]
async fn bad_peer_inside_subtree_records_resolved_from_parent() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "1.0.0".to_string()),
        fake_result(
            "foo",
            "1.0.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "dep": "1.0.0", "bar": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("dep".to_string(), "1.0.0".to_string()),
        fake_result("dep", "1.0.0", serde_json::json!({ "name": "dep", "version": "1.0.0" })),
    );
    table.insert(
        ("bar".to_string(), "1.0.0".to_string()),
        fake_result(
            "bar",
            "1.0.0",
            serde_json::json!({
                "name": "bar",
                "version": "1.0.0",
                "peerDependencies": { "dep": "10.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "1.0.0" }));

    let mut tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    assert!(
        result.peer_dependency_issues.bad.contains_key("dep"),
        "expected bad peer issue for dep, got {:?}",
        result.peer_dependency_issues,
    );
    let bad = &result.peer_dependency_issues.bad["dep"];
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].found_version, "1.0.0");
    assert_eq!(bad[0].wanted_range, "10.0.0");
}

/// A child that declares only peer dependencies is non-leaf while
/// its `children_by_id` entry stays empty — peers are hoisted to the
/// importer rather than walked as edges. It must keep the non-leaf
/// classification the eager walk picked when it's reached again
/// through a lazy revisit. Regression for the divergence between
/// `pkg_is_leaf` and the inferred `children_by_id.is_empty()` check
/// the lazy realizer used before `ResolvedPackage::is_leaf` was
/// persisted.
#[tokio::test]
async fn revisit_with_peer_only_child_keeps_per_occurrence_node_id() {
    use crate::node_id::NodeId;
    let mut table = HashMap::default();
    // Two siblings that both depend on `parent`, so `parent` is
    // walked once eagerly and revisited via the second sibling
    // (the revisit goes through the lazy children path).
    table.insert(
        ("p1".to_string(), "^1.0.0".to_string()),
        fake_result(
            "p1",
            "1.0.0",
            serde_json::json!({
                "name": "p1",
                "version": "1.0.0",
                "dependencies": { "parent": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("p2".to_string(), "^1.0.0".to_string()),
        fake_result(
            "p2",
            "1.0.0",
            serde_json::json!({
                "name": "p2",
                "version": "1.0.0",
                "dependencies": { "parent": "^1.0.0" }
            }),
        ),
    );
    // `parent` has a peer dep so the peer resolver descends into
    // every occurrence (purePkgs would otherwise short-circuit
    // before realize_children runs).
    table.insert(
        ("parent".to_string(), "^1.0.0".to_string()),
        fake_result(
            "parent",
            "1.0.0",
            serde_json::json!({
                "name": "parent",
                "version": "1.0.0",
                "dependencies": { "peer-only": "^1.0.0" },
                "peerDependencies": { "peer": "^1.0.0" }
            }),
        ),
    );
    // `peer-only` declares a peer and nothing else, so its
    // `children_by_id` entry stays empty. Without persistence, the
    // lazy realizer would read that emptiness as leaf-ness and
    // collapse the revisit onto `NodeId::Leaf`.
    table.insert(
        ("peer-only".to_string(), "^1.0.0".to_string()),
        fake_result(
            "peer-only",
            "1.0.0",
            serde_json::json!({
                "name": "peer-only",
                "version": "1.0.0",
                "peerDependencies": { "peer": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("peer".to_string(), "^1.0.0".to_string()),
        fake_result("peer", "1.0.0", serde_json::json!({ "name": "peer", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "p1": "^1.0.0",
        "p2": "^1.0.0",
        "peer": "^1.0.0",
    }));

    let mut tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    // First-walk classification must be non-leaf — `pkg_is_leaf`
    // counts a declared peer as a child.
    assert!(
        !tree.packages.get("peer-only@1.0.0").expect("peer-only resolved").is_leaf,
        "a package declaring a peer must keep is_leaf=false (eager walker contract)",
    );

    resolve_peers(&mut tree, ResolvePeersOptions::default());

    // After lazy realization, every occurrence of `peer-only` must
    // use a Counter NodeId — the same shape the eager walker
    // assigned on first visit. A `Leaf` NodeId here would mean
    // `realize_children` misclassified the package and collapsed
    // distinct occurrences, breaking per-call-site state for any
    // future visitor that descends through it.
    let peer_only_node_ids: Vec<&NodeId> = tree
        .dependencies_tree
        .iter()
        .filter(|(_, node)| node.resolved_package_id == "peer-only@1.0.0".into())
        .map(|(id, _)| id)
        .collect();
    assert!(!peer_only_node_ids.is_empty(), "expected at least one tree entry for peer-only");
    for id in &peer_only_node_ids {
        assert!(
            matches!(id, NodeId::Counter(_)),
            "a peer-declaring child must use Counter NodeId in every occurrence, got {id:?}",
        );
    }
}

/// The path to an external link is not added to the lockfile when
/// it resolves a peer dependency. Narrowed to the peer-resolution
/// slice.
#[tokio::test]
async fn external_link_peer_remaps_to_node_modules_when_exclude_links_on() {
    use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
    use pnpm_resolving_resolver_base::PkgResolutionId;

    let link_id = "link:/abs/external";
    let mut table = HashMap::default();
    table.insert(
        ("abc".to_string(), "1.0.0".to_string()),
        fake_result(
            "abc",
            "1.0.0",
            serde_json::json!({
                "name": "abc",
                "version": "1.0.0",
                "peerDependencies": { "peer-a": "*" },
            }),
        ),
    );
    // `link:` direct dep — the local resolver normally fills this
    // shape; the tests stub it out directly. `name_ver = None`
    // matches the local resolver's behavior (the package name is
    // read from the manifest, not the id).
    table.insert(
        ("peer-a".to_string(), "link:/abs/external".to_string()),
        pnpm_resolving_resolver_base::ResolveResult {
            id: PkgResolutionId::from(link_id.to_string()),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(std::sync::Arc::new(
                serde_json::json!({ "name": "peer-a", "version": "1.0.0" }),
            )),
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "/abs/external".to_string(),
            }),
            resolved_via: "local-filesystem".to_string(),
            normalized_bare_specifier: None,
            alias: Some("peer-a".to_string()),
            policy_violation: None,
        },
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "abc": "1.0.0",
        "peer-a": "link:/abs/external",
    }));
    let mut tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .expect("resolve tree");

    let lockfile_dir = std::path::PathBuf::from("/tmp/lockfile-dir");
    let modules_dir = lockfile_dir.join("node_modules");
    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions {
            peers_suffix_max_length: 1000,
            dedupe_peers: false,
            exclude_links_from_lockfile: true,
            lockfile_dir: Some(lockfile_dir),
            modules_dir: Some(modules_dir),
            ..ResolvePeersOptions::default()
        },
    );

    let abc_dep_path =
        result.direct_dependencies_by_alias.get("abc").cloned().expect("abc is a direct dep");
    assert_eq!(
        abc_dep_path,
        DepPath::from("abc@1.0.0(peer-a@node_modules+peer-a)".to_string()),
        "abc's peer suffix encodes `<modules_dir-relative>/<alias>` via link_path_to_peer_version",
    );
    let abc_node = result.graph.get(&abc_dep_path).expect("abc node in graph");
    let peer_child = abc_node.children.get("peer-a").expect("abc snapshot has a peer-a child edge");
    assert_eq!(
        peer_child,
        &DepPath::from("link:node_modules/peer-a".to_string()),
        "snapshot child ref reuses the remapped link node id verbatim",
    );
}
