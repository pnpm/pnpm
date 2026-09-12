use super::{
    DepPath, DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions,
    ResolvePeersOptions, StubResolver, assert_eq, fake_manifest, fake_result,
    resolve_dependency_tree, resolve_emotion_fixture, resolve_peers,
};

/// `p → q → p` is a cycle whose re-entry of `p` resolves against truncated
/// children: with `q` dropped to break the cycle, that occurrence of `p`
/// looks peer-free. It must not be cached as pure, or the sibling occurrence
/// reached through `w` short-circuits and loses the optional transitive peer
/// `e` that `q` declares — churning the lockfile by traversal order.
/// <https://github.com/pnpm/pnpm/issues/5108>
#[tokio::test]
async fn cycle_reentry_does_not_drop_sibling_occurrence_transitive_peers() {
    let mut table = HashMap::default();
    for (name, manifest) in [
        (
            "p",
            serde_json::json!({ "name": "p", "version": "1.0.0", "dependencies": { "q": "1.0.0" } }),
        ),
        (
            "q",
            serde_json::json!({ "name": "q", "version": "1.0.0", "dependencies": { "p": "1.0.0" }, "peerDependencies": { "e": "1.0.0" }, "peerDependenciesMeta": { "e": { "optional": true } } }),
        ),
        (
            "w",
            serde_json::json!({ "name": "w", "version": "1.0.0", "dependencies": { "p": "1.0.0" } }),
        ),
    ] {
        table.insert((name.to_string(), "1.0.0".to_string()), fake_result(name, "1.0.0", manifest));
    }
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "p": "1.0.0", "w": "1.0.0" }));
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
    assert!(tree.all_peer_dep_names.contains("e"));

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    for name in ["p@1.0.0", "w@1.0.0"] {
        let entry = result.graph.get(&DepPath::from(name.to_string())).expect("entry in graph");
        assert!(
            entry.transitive_peer_dependencies.contains("e"),
            "{name} should carry transitive peer 'e', got {:?}",
            entry.transitive_peer_dependencies,
        );
    }
}

#[tokio::test]
async fn peer_resolved_against_sibling_at_parent_level() {
    let mut table = HashMap::default();
    table.insert(
        ("react".to_string(), "18.0.0".to_string()),
        fake_result("react", "18.0.0", serde_json::json!({ "name": "react", "version": "18.0.0" })),
    );
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "react": "18.0.0", "react-dom": "18.0.0" }));
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
    assert!(tree.all_peer_dep_names.contains("react"));

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let react_dom_dep_path = result
        .direct_dependencies_by_alias
        .get("react-dom")
        .cloned()
        .expect("react-dom is a direct dep");
    assert_eq!(react_dom_dep_path, DepPath::from("react-dom@18.0.0(react@18.0.0)".to_string()));
    assert_eq!(
        result.direct_dependencies_by_alias.get("react"),
        Some(&DepPath::from("react@18.0.0".to_string())),
    );
    assert!(result.peer_dependency_issues.missing.is_empty());
    assert!(result.peer_dependency_issues.bad.is_empty());
}

#[tokio::test]
async fn bad_peer_version_is_reported() {
    let mut table = HashMap::default();
    table.insert(
        ("react".to_string(), "17.0.0".to_string()),
        fake_result("react", "17.0.0", serde_json::json!({ "name": "react", "version": "17.0.0" })),
    );
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "react": "17.0.0", "react-dom": "18.0.0" }));
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
    assert!(result.peer_dependency_issues.bad.contains_key("react"));
    let bad = &result.peer_dependency_issues.bad["react"];
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].found_version, "17.0.0");
    assert_eq!(bad[0].wanted_range, "^18.0.0");
    assert_eq!(
        result.direct_dependencies_by_alias.get("react-dom"),
        Some(&DepPath::from("react-dom@18.0.0(react@17.0.0)".to_string())),
    );
}

/// `dedupePeers: true` collapses recursive peer suffixes into
/// version-only identifiers. Without `dedupePeers`, a peer whose
/// resolution already carries a peer suffix (e.g. `@emotion/react`
/// resolved its own `react` peer first) leaks its nested suffix
/// into the consumer's depPath:
/// `@emotion/styled@11.0.0(@emotion/react@11.0.0(react@18.0.0))(react@18.0.0)`.
/// With `dedupePeers` on, the peer-id is `name@version` instead, so
/// the consumer's suffix stays flat:
/// `@emotion/styled@11.0.0(@emotion/react@11.0.0)(react@18.0.0)`.
#[tokio::test]
async fn dedupe_peers_collapses_nested_peer_suffixes() {
    let result = resolve_emotion_fixture(ResolvePeersOptions {
        dedupe_peers: true,
        ..ResolvePeersOptions::default()
    })
    .await;
    let mut keys: Vec<String> = result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "@emotion/react@11.0.0(react@18.0.0)".to_string(),
            "@emotion/styled@11.0.0(@emotion/react@11.0.0)(react@18.0.0)".to_string(),
            "react@18.0.0".to_string(),
        ],
    );
}

/// Opposite of [`dedupe_peers_collapses_nested_peer_suffixes`] — the
/// same fixture under `dedupePeers: false` keeps the nested peer
/// suffix on `@emotion/styled`'s depPath, proving the dedupe
/// branch is the only thing flipping the rendering.
#[tokio::test]
async fn no_dedupe_peers_keeps_nested_peer_suffixes() {
    let result = resolve_emotion_fixture(ResolvePeersOptions::default()).await;
    let mut keys: Vec<String> = result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "@emotion/react@11.0.0(react@18.0.0)".to_string(),
            "@emotion/styled@11.0.0(@emotion/react@11.0.0(react@18.0.0))(react@18.0.0)".to_string(),
            "react@18.0.0".to_string(),
        ],
    );
}

/// Transitive peer: `a` depends on `b`, `b` has peer `c`, importer
/// has direct `a` + `c`. Even though `a` has no peers itself, its
/// child `b` carries `c` as an external peer, so `a` propagates `c`
/// up to its own depPath suffix too. Both `a` and `b` land as
/// `…(c@1.0.0)`.
///
/// Pacquet's [`DepPath`] uses `name@version` for pure packages, so
/// the `dedupe_peers=true` vs `false` rendering of a pure peer is
/// byte-identical (both produce `(c@1.0.0)`). The contract this
/// test locks down is the transitive-peer propagation itself, not
/// the byte shape of the peer-id.
#[tokio::test]
async fn dedupe_peers_propagates_transitive_peer_to_parent() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "b": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            serde_json::json!({
                "name": "b",
                "version": "1.0.0",
                "peerDependencies": { "c": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("c".to_string(), "1.0.0".to_string()),
        fake_result("c", "1.0.0", serde_json::json!({ "name": "c", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "1.0.0", "c": "1.0.0" }));
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

    let result = resolve_peers(
        &mut tree,
        ResolvePeersOptions { dedupe_peers: true, ..ResolvePeersOptions::default() },
    );
    let mut keys: Vec<String> = result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["a@1.0.0(c@1.0.0)".to_string(), "b@1.0.0(c@1.0.0)".to_string(), "c@1.0.0".to_string(),],
    );
}

/// A peer's own peer is shared with a sibling that peer-depends both.
/// `plugin` peer-depends both `parser` and `typescript`; `parser`
/// peer-depends `typescript`. `plugin` and `parser` live under
/// `umbrella` (under `app`, which also brings `typescript@1.0.0`), while
/// the importer also has a top-level `typescript@2.0.0` + `parser@1.0.0`.
/// `plugin`'s `parser` must resolve `typescript@1.0.0` — the version
/// `plugin` itself uses — not be shadowed by the top-level `parser` that
/// resolved `typescript@2.0.0`. Exercises the depPath suffix machinery.
#[tokio::test]
async fn peers_own_peer_shared_with_sibling_that_peer_depends_both() {
    let mut table = HashMap::default();
    for version in ["1.0.0", "2.0.0"] {
        table.insert(
            ("typescript".to_string(), version.to_string()),
            fake_result(
                "typescript",
                version,
                serde_json::json!({ "name": "typescript", "version": version }),
            ),
        );
    }
    table.insert(
        ("parser".to_string(), "1.0.0".to_string()),
        fake_result(
            "parser",
            "1.0.0",
            serde_json::json!({
                "name": "parser",
                "version": "1.0.0",
                "peerDependencies": { "typescript": "*" }
            }),
        ),
    );
    table.insert(
        ("plugin".to_string(), "1.0.0".to_string()),
        fake_result(
            "plugin",
            "1.0.0",
            serde_json::json!({
                "name": "plugin",
                "version": "1.0.0",
                "peerDependencies": { "parser": "*", "typescript": "*" }
            }),
        ),
    );
    table.insert(
        ("umbrella".to_string(), "1.0.0".to_string()),
        fake_result(
            "umbrella",
            "1.0.0",
            serde_json::json!({
                "name": "umbrella",
                "version": "1.0.0",
                "dependencies": { "plugin": "1.0.0", "parser": "1.0.0" },
                "peerDependencies": { "typescript": "*" }
            }),
        ),
    );
    table.insert(
        ("app".to_string(), "1.0.0".to_string()),
        fake_result(
            "app",
            "1.0.0",
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "typescript": "1.0.0", "umbrella": "1.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({
        "typescript": "2.0.0",
        "parser": "1.0.0",
        "app": "1.0.0",
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
    let keys: Vec<String> = result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
    assert!(
        keys.contains(
            &"plugin@1.0.0(parser@1.0.0(typescript@1.0.0))(typescript@1.0.0)".to_string()
        ),
        "plugin's parser must resolve typescript@1.0.0; got: {keys:?}",
    );
    assert!(
        !keys.contains(
            &"plugin@1.0.0(parser@1.0.0(typescript@2.0.0))(typescript@1.0.0)".to_string()
        ),
        "plugin's parser must not be shadowed by the top-level typescript@2.0.0: {keys:?}",
    );
}

/// A peer that is a walk-ancestor must still carry its own peer
/// suffix in the dependent's depPath. `a` is a direct dep with peer
/// `c`; its child `b` peer-depends on `a`. While `b`'s suffix is
/// being built, `a` is mid-walk (in-progress) so its depPath isn't
/// finalized yet. The post-walk [`build_final_dep_paths`] pass must
/// resolve `a` to its full `a@1.0.0(c@1.0.0)` — not the collapsed
/// `a@1.0.0` the cycle fallback would emit (pnpm only collapses
/// genuine cycles, and `a→b→a` here resolves because `a` and `b`
/// don't form a peer-graph SCC). Regression test for
/// <https://github.com/pnpm/pnpm/issues/12266>.
#[tokio::test]
async fn ancestor_peer_carries_its_own_suffix() {
    let mut table = HashMap::default();
    table.insert(
        ("a".to_string(), "1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "b": "1.0.0" },
                "peerDependencies": { "c": "*" }
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            serde_json::json!({
                "name": "b",
                "version": "1.0.0",
                "peerDependencies": { "a": "*" }
            }),
        ),
    );
    table.insert(
        ("c".to_string(), "1.0.0".to_string()),
        fake_result("c", "1.0.0", serde_json::json!({ "name": "c", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "1.0.0", "c": "1.0.0" }));
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
    let mut keys: Vec<String> = result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "a@1.0.0(c@1.0.0)".to_string(),
            "b@1.0.0(a@1.0.0(c@1.0.0))".to_string(),
            "c@1.0.0".to_string(),
        ],
    );

    // `c` is `a`'s peer, not `b`'s — it must not leak into `b`'s
    // dependencies (only `b`'s own peer `a` is a child of `b`).
    let b_node = &result.graph[&DepPath::from("b@1.0.0(a@1.0.0(c@1.0.0))".to_string())];
    let b_children: Vec<&str> = b_node.children.keys().map(String::as_str).collect();
    assert_eq!(b_children, vec!["a"]);
}

/// Regression test for the post-walk peer-edge patch. With manifest
/// order `{ react-dom: …, react: … }`, react-dom is walked before
/// react and the peer's depPath isn't known yet at the time
/// `graph_children` is built. The post-pass has to patch the edge
/// in so the install layer's recursion finds react when descending
/// into react-dom's slot.
#[tokio::test]
async fn peer_edge_is_patched_when_peer_walked_after_consumer() {
    let mut table = HashMap::default();
    table.insert(
        ("react-dom".to_string(), "18.0.0".to_string()),
        fake_result(
            "react-dom",
            "18.0.0",
            serde_json::json!({
                "name": "react-dom",
                "version": "18.0.0",
                "peerDependencies": { "react": "^18.0.0" }
            }),
        ),
    );
    table.insert(
        ("react".to_string(), "18.0.0".to_string()),
        fake_result("react", "18.0.0", serde_json::json!({ "name": "react", "version": "18.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    // Manifest order puts react-dom first.
    let (_tmp, manifest) =
        fake_manifest(serde_json::json!({ "react-dom": "18.0.0", "react": "18.0.0" }));
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
    let react_dom_dep_path = result
        .direct_dependencies_by_alias
        .get("react-dom")
        .cloned()
        .expect("react-dom is a direct dep");
    let node = result.graph.get(&react_dom_dep_path).expect("graph entry for react-dom");
    // Without the post-pass, this edge would be missing because
    // `node_dep_paths` doesn't yet contain react when react-dom is
    // being walked.
    assert_eq!(node.children.get("react"), Some(&DepPath::from("react@18.0.0".to_string())));
}

/// Cyclic peer dependencies: `foo` peer-depends on `qar` and `zoo`,
/// `bar` peer-depends on `foo` and `zoo`, `qar` peer-depends on
/// `foo` and `bar`, `zoo` peer-depends on `qar`. The walker breaks
/// the cycle and every node lands in the graph with the right
/// peer suffix.
#[tokio::test]
async fn cyclic_peer_dependencies_resolve_cleanly() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "1.0.0".to_string()),
        fake_result(
            "foo",
            "1.0.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "dependencies": { "bar": "1.0.0" },
                "peerDependencies": { "qar": "1.0.0", "zoo": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "1.0.0".to_string()),
        fake_result(
            "bar",
            "1.0.0",
            serde_json::json!({
                "name": "bar",
                "version": "1.0.0",
                "dependencies": { "qar": "1.0.0" },
                "peerDependencies": { "foo": "1.0.0", "zoo": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("qar".to_string(), "1.0.0".to_string()),
        fake_result(
            "qar",
            "1.0.0",
            serde_json::json!({
                "name": "qar",
                "version": "1.0.0",
                "dependencies": { "zoo": "1.0.0" },
                "peerDependencies": { "foo": "1.0.0", "bar": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("zoo".to_string(), "1.0.0".to_string()),
        fake_result(
            "zoo",
            "1.0.0",
            serde_json::json!({
                "name": "zoo",
                "version": "1.0.0",
                "dependencies": { "foo": "1.0.0", "bar": "1.0.0" },
                "peerDependencies": { "qar": "1.0.0" }
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    // Importer carries `foo` (auto-install-peers off here — we
    // exercise the peer matcher, not the hoister). With
    // auto-install-peers the qar/zoo/bar peers would get hoisted
    // to the importer level; for this test we accept the
    // resulting missing-peer issues and just verify the graph
    // closure and no panics.
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

    assert!(tree.packages.contains_key("foo@1.0.0"));
    assert!(tree.packages.contains_key("bar@1.0.0"));
    assert!(tree.packages.contains_key("qar@1.0.0"));
    assert!(tree.packages.contains_key("zoo@1.0.0"));

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

    // Every resolved package surfaces a graph entry, even though
    // their peers form a cycle. The exact peer-suffix shape is
    // sensitive to walk order; the important invariant is that
    // every depPath starts with the expected pkg id.
    let dep_paths: Vec<String> = result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
    for (name, _) in &[("foo", ""), ("bar", ""), ("qar", ""), ("zoo", "")] {
        let prefix = format!("{name}@1.0.0");
        assert!(
            dep_paths.iter().any(|dp| dp.starts_with(&prefix)),
            "no graph entry starts with {prefix}: {dep_paths:?}",
        );
    }
}

/// Same package reached via two parent chains where the peer
/// resolves only via one: both occurrences must land in the
/// graph with distinct depPaths. When a package is referenced
/// twice and one occurrence cannot resolve its peers, the other
/// occurrence is still resolved.
#[tokio::test]
async fn revisit_resolves_peer_in_one_occurrence_misses_in_other() {
    let mut table = HashMap::default();
    table.insert(
        ("zoo".to_string(), "1.0.0".to_string()),
        fake_result(
            "zoo",
            "1.0.0",
            serde_json::json!({
                "name": "zoo",
                "version": "1.0.0",
                "dependencies": { "foo": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "1.0.0".to_string()),
        fake_result(
            "bar",
            "1.0.0",
            serde_json::json!({
                "name": "bar",
                "version": "1.0.0",
                "dependencies": { "zoo": "1.0.0", "qar": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("foo".to_string(), "1.0.0".to_string()),
        fake_result(
            "foo",
            "1.0.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.0.0",
                "peerDependencies": { "qar": "1.0.0" }
            }),
        ),
    );
    table.insert(
        ("qar".to_string(), "1.0.0".to_string()),
        fake_result("qar", "1.0.0", serde_json::json!({ "name": "qar", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    // Root depends on zoo (direct: foo's qar peer is missing) and
    // bar (transitive: foo's qar peer resolves via bar's qar
    // sibling).
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "zoo": "1.0.0", "bar": "1.0.0" }));

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

    let dep_paths: std::collections::HashSet<String> =
        result.graph.keys().map(|dp| dp.as_str().to_string()).collect();

    assert!(
        dep_paths.contains("foo@1.0.0"),
        "missing-peer occurrence of foo missing from graph: {dep_paths:?}",
    );
    assert!(
        dep_paths.contains("foo@1.0.0(qar@1.0.0)"),
        "resolved-peer occurrence of foo missing from graph: {dep_paths:?}",
    );

    assert!(dep_paths.contains("bar@1.0.0"), "{dep_paths:?}");
    assert!(dep_paths.contains("qar@1.0.0"), "{dep_paths:?}");
    assert!(dep_paths.contains("zoo@1.0.0"), "direct zoo (no peer suffix) missing: {dep_paths:?}");
    assert!(
        dep_paths.contains("zoo@1.0.0(qar@1.0.0)"),
        "transitive zoo (qar peer bubbled up) missing: {dep_paths:?}",
    );

    assert!(
        result.peer_dependency_issues.missing.contains_key("qar"),
        "expected missing qar peer issue, got {:?}",
        result.peer_dependency_issues.missing.keys().collect::<Vec<_>>(),
    );
}
