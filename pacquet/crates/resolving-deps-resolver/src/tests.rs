use std::{collections::HashMap, str::FromStr, sync::Mutex};

use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;

use crate::resolve_dependency_tree::{
    ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
};

/// Stub resolver fed from a `(name, range)` → `ResolveResult` map.
/// Records each `(name, range)` query so tests can assert dedup.
struct StubResolver {
    table: HashMap<(String, String), ResolveResult>,
    calls: Mutex<Vec<(String, String)>>,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let key = (
            wanted.alias.clone().unwrap_or_default(),
            wanted.bare_specifier.clone().unwrap_or_default(),
        );
        self.calls.lock().unwrap().push(key.clone());
        let result = self.table.get(&key).cloned();
        Box::pin(async move { Ok::<_, ResolveError>(result) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

fn fake_result(name: &str, version: &str, manifest: serde_json::Value) -> ResolveResult {
    use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: Some(version.to_string()),
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{name}-{version}.tgz"),
            integrity: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    }
}

fn fake_manifest(root_deps: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    let json = serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": root_deps,
    });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

#[tokio::test]
async fn walks_dependencies_and_builds_flat_tree() {
    let mut table = HashMap::new();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result(
            "foo",
            "1.2.0",
            serde_json::json!({
                "name": "foo",
                "version": "1.2.0",
                "dependencies": { "bar": "^2.0.0" }
            }),
        ),
    );
    table.insert(
        ("bar".to_string(), "^2.0.0".to_string()),
        fake_result(
            "bar",
            "2.3.0",
            serde_json::json!({
                "name": "bar",
                "version": "2.3.0",
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct.len(), 1);
    assert_eq!(tree.direct[0].alias, "foo");
    assert_eq!(tree.direct[0].id, "foo@1.2.0");
    assert_eq!(tree.packages.len(), 2);
    assert!(tree.packages.contains_key("foo@1.2.0"));
    let foo_node_id = &tree.direct[0].node_id;
    let foo_tree_node = tree.dependencies_tree.get(foo_node_id).unwrap();
    assert_eq!(foo_tree_node.children.len(), 1);
    let bar_node_id = foo_tree_node.children.get("bar").unwrap();
    let bar_tree_node = tree.dependencies_tree.get(bar_node_id).unwrap();
    assert_eq!(bar_tree_node.resolved_package_id, "bar@2.3.0");
    assert!(tree.policy_violations.is_empty());
}

#[tokio::test]
async fn dedupes_when_the_same_package_appears_in_two_subtrees() {
    let mut table = HashMap::new();
    table.insert(
        ("a".to_string(), "^1.0.0".to_string()),
        fake_result(
            "a",
            "1.0.0",
            serde_json::json!({
                "name": "a",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("b".to_string(), "^1.0.0".to_string()),
        fake_result(
            "b",
            "1.0.0",
            serde_json::json!({
                "name": "b",
                "version": "1.0.0",
                "dependencies": { "shared": "^1.0.0" }
            }),
        ),
    );
    table.insert(
        ("shared".to_string(), "^1.0.0".to_string()),
        fake_result(
            "shared",
            "1.0.0",
            serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "^1.0.0", "b": "^1.0.0" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.packages.len(), 3);
    assert!(tree.packages.contains_key("a@1.0.0"));
    assert!(tree.packages.contains_key("b@1.0.0"));
    assert!(tree.packages.contains_key("shared@1.0.0"));

    // `shared` is a leaf (no deps / peers); both `a` and `b` must
    // point at the same `NodeId` and the tree must carry exactly one
    // `shared` entry. Mirrors upstream's
    // [`pkgIsLeaf` reuse](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1580).
    let a_tree = tree.dependencies_tree.get(&tree.direct[0].node_id).unwrap();
    let b_tree = tree.dependencies_tree.get(&tree.direct[1].node_id).unwrap();
    let shared_via_a = a_tree.children.get("shared").unwrap();
    let shared_via_b = b_tree.children.get("shared").unwrap();
    assert_eq!(shared_via_a, shared_via_b);
    let shared_occurrences =
        tree.dependencies_tree.values().filter(|n| n.resolved_package_id == "shared@1.0.0").count();
    assert_eq!(shared_occurrences, 1);
}

/// A chain that declines every spec (every `resolve()` returns
/// `Ok(None)`) must NOT silently drop the edge — that would leave
/// installs missing transitive deps and report success. The walker
/// surfaces `SpecNotSupported` with the offending specifier
/// rendered the way upstream's `default-resolver` does, so callers
/// can produce the same `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`
/// diagnostic the chain dispatcher does.
#[tokio::test]
async fn declined_specifier_surfaces_spec_not_supported_error() {
    let resolver = StubResolver { table: HashMap::new(), calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "git+ssh://example.com" }));

    let err = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
        },
    )
    .await
    .expect_err("declined spec must error");
    match err {
        ResolveDependencyTreeError::SpecNotSupported { specifier } => {
            assert_eq!(specifier, "foo@git+ssh://example.com");
        }
        other => panic!("expected SpecNotSupported, got {other:?}"),
    }
}

mod block_exotic_subdeps {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use pacquet_package_manifest::DependencyGroup;
    use pacquet_resolving_resolver_base::ResolveOptions;

    use super::{StubResolver, fake_manifest, fake_result};
    use crate::resolve_dependency_tree::{
        ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
    };

    fn git_result(
        name: &str,
        version: &str,
        manifest: serde_json::Value,
    ) -> pacquet_resolving_resolver_base::ResolveResult {
        let mut result = fake_result(name, version, manifest);
        result.resolved_via = "git-repository".to_string();
        result
    }

    /// A transitive dep resolved via an exotic protocol fails the
    /// install when `block_exotic_subdeps` is on. Mirrors upstream's
    /// `EXOTIC_SUBDEP` error.
    #[tokio::test]
    async fn rejects_exotic_transitive_dep() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result(
                "foo",
                "1.0.0",
                serde_json::json!({
                    "name": "foo",
                    "version": "1.0.0",
                    "dependencies": { "say-hi": "github:zkochan/hi" }
                }),
            ),
        );
        table.insert(
            ("say-hi".to_string(), "github:zkochan/hi".to_string()),
            git_result(
                "say-hi",
                "1.0.0",
                serde_json::json!({ "name": "say-hi", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        let err = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions {
                    block_exotic_subdeps: true,
                    ..ResolveOptions::default()
                },
                patched_dependencies: None,
            },
        )
        .await
        .expect_err("exotic subdep must error");
        match err {
            ResolveDependencyTreeError::ExoticSubdep { specifier, resolved_via } => {
                assert_eq!(specifier, "say-hi");
                assert_eq!(resolved_via, "git-repository");
            }
            other => panic!("expected ExoticSubdep, got {other:?}"),
        }
    }

    /// An exotic *direct* dep still resolves — the gate only fires
    /// past the importer.
    #[tokio::test]
    async fn allows_exotic_direct_dep() {
        let mut table = HashMap::new();
        table.insert(
            ("is-negative".to_string(), "kevva/is-negative#1.0.0".to_string()),
            git_result(
                "is-negative",
                "1.0.0",
                serde_json::json!({ "name": "is-negative", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) =
            fake_manifest(serde_json::json!({ "is-negative": "kevva/is-negative#1.0.0" }));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions {
                    block_exotic_subdeps: true,
                    ..ResolveOptions::default()
                },
                patched_dependencies: None,
            },
        )
        .await
        .expect("direct exotic dep should resolve");
        assert_eq!(tree.direct.len(), 1);
        assert_eq!(tree.direct[0].alias, "is-negative");
    }

    /// A registry subdep is fine when the gate is on.
    #[tokio::test]
    async fn allows_registry_subdep() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result(
                "foo",
                "1.0.0",
                serde_json::json!({
                    "name": "foo",
                    "version": "1.0.0",
                    "dependencies": { "bar": "^2.0.0" }
                }),
            ),
        );
        table.insert(
            ("bar".to_string(), "^2.0.0".to_string()),
            fake_result("bar", "2.0.0", serde_json::json!({ "name": "bar", "version": "2.0.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions {
                    block_exotic_subdeps: true,
                    ..ResolveOptions::default()
                },
                patched_dependencies: None,
            },
        )
        .await
        .expect("registry subdep must pass");
    }

    /// With the gate off, an exotic subdep walks like any other.
    #[tokio::test]
    async fn allows_exotic_subdep_when_disabled() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result(
                "foo",
                "1.0.0",
                serde_json::json!({
                    "name": "foo",
                    "version": "1.0.0",
                    "dependencies": { "say-hi": "github:zkochan/hi" }
                }),
            ),
        );
        table.insert(
            ("say-hi".to_string(), "github:zkochan/hi".to_string()),
            git_result(
                "say-hi",
                "1.0.0",
                serde_json::json!({ "name": "say-hi", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions {
                    block_exotic_subdeps: false,
                    ..ResolveOptions::default()
                },
                patched_dependencies: None,
            },
        )
        .await
        .expect("exotic subdep must pass when disabled");
        assert!(tree.packages.contains_key("say-hi@1.0.0"));
    }
}

mod peers {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use pacquet_package_manifest::DependencyGroup;
    use pacquet_resolving_resolver_base::ResolveOptions;
    use pretty_assertions::assert_eq;

    use super::{StubResolver, fake_manifest, fake_result};
    use crate::resolve_dependency_tree::{ResolveDependencyTreeOptions, resolve_dependency_tree};
    use crate::resolve_peers::{ResolvePeersOptions, resolve_peers};
    use pacquet_deps_path::DepPath;

    /// A pure leaf — no peer dependencies — should land in the graph
    /// with its depPath equal to its pkgIdWithPatchHash.
    #[tokio::test]
    async fn pure_package_has_dep_path_equal_to_pkg_id() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));
        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());
        assert_eq!(
            result.direct_dependencies_by_alias.get("foo"),
            Some(&DepPath::from("foo@1.0.0".to_string())),
        );
        assert!(result.peer_dependency_issues.missing.is_empty());
        assert!(result.peer_dependency_issues.bad.is_empty());
    }

    /// `parent → child` where `child` declares `react` as a peer and
    /// `parent` also depends on `react`: the peer resolves against the
    /// sibling, and `child`'s depPath gains a `(react@…)` suffix.
    #[tokio::test]
    async fn peer_resolved_against_sibling_at_parent_level() {
        let mut table = HashMap::new();
        table.insert(
            ("react".to_string(), "18.0.0".to_string()),
            fake_result(
                "react",
                "18.0.0",
                serde_json::json!({ "name": "react", "version": "18.0.0" }),
            ),
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
        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();
        assert!(tree.all_peer_dep_names.contains("react"));

        let result = resolve_peers(&tree, ResolvePeersOptions::default());
        let react_dom_dep_path = result
            .direct_dependencies_by_alias
            .get("react-dom")
            .cloned()
            .expect("react-dom is a direct dep");
        assert_eq!(react_dom_dep_path, DepPath::from("react-dom@18.0.0(react@18.0.0)".to_string()));
        // react itself stays pure.
        assert_eq!(
            result.direct_dependencies_by_alias.get("react"),
            Some(&DepPath::from("react@18.0.0".to_string())),
        );
        assert!(result.peer_dependency_issues.missing.is_empty());
        assert!(result.peer_dependency_issues.bad.is_empty());
    }

    /// Missing peer: `react-dom` requires `react` but the importer
    /// doesn't include it. We expect an issue + no resolved peer.
    #[tokio::test]
    async fn missing_peer_is_reported() {
        let mut table = HashMap::new();
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
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "react-dom": "18.0.0" }));
        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());
        assert!(result.peer_dependency_issues.missing.contains_key("react"));
        // No resolved peer ⇒ react-dom stays pure.
        assert_eq!(
            result.direct_dependencies_by_alias.get("react-dom"),
            Some(&DepPath::from("react-dom@18.0.0".to_string())),
        );
    }

    /// Bad peer: the importer carries `react@17` but `react-dom@18`
    /// requires `react@^18`. An issue surfaces; the peer is still
    /// recorded as resolved (the pick is intentional, the warning is
    /// informational).
    #[tokio::test]
    async fn bad_peer_version_is_reported() {
        let mut table = HashMap::new();
        table.insert(
            ("react".to_string(), "17.0.0".to_string()),
            fake_result(
                "react",
                "17.0.0",
                serde_json::json!({ "name": "react", "version": "17.0.0" }),
            ),
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
        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());
        assert!(result.peer_dependency_issues.bad.contains_key("react"));
        let bad = &result.peer_dependency_issues.bad["react"];
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].found_version, "17.0.0");
        assert_eq!(bad[0].wanted_range, "^18.0.0");
        // Peer-suffix uses the resolved (17.0.0) version — the install
        // proceeds with the picked candidate.
        assert_eq!(
            result.direct_dependencies_by_alias.get("react-dom"),
            Some(&DepPath::from("react-dom@18.0.0(react@17.0.0)".to_string())),
        );
    }

    /// Regression test for the post-walk peer-edge patch. With manifest
    /// order `{ react-dom: …, react: … }`, react-dom is walked before
    /// react and the peer's depPath isn't known yet at the time
    /// `graph_children` is built. The post-pass has to patch the edge
    /// in so the install layer's recursion finds react when descending
    /// into react-dom's slot.
    #[tokio::test]
    async fn peer_edge_is_patched_when_peer_walked_after_consumer() {
        let mut table = HashMap::new();
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
            fake_result(
                "react",
                "18.0.0",
                serde_json::json!({ "name": "react", "version": "18.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        // Manifest order puts react-dom first.
        let (_tmp, manifest) =
            fake_manifest(serde_json::json!({ "react-dom": "18.0.0", "react": "18.0.0" }));
        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());
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
    /// peer suffix. Ports upstream's `'resolve peer dependencies of
    /// cyclic dependencies'` (installing/deps-resolver/test/resolvePeers.ts:14).
    #[tokio::test]
    async fn cyclic_peer_dependencies_resolve_cleanly() {
        let mut table = HashMap::new();
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

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        // Every package appears in `packages` exactly once — cycle
        // break did not duplicate or drop anything.
        assert!(tree.packages.contains_key("foo@1.0.0"));
        assert!(tree.packages.contains_key("bar@1.0.0"));
        assert!(tree.packages.contains_key("qar@1.0.0"));
        assert!(tree.packages.contains_key("zoo@1.0.0"));

        // Peer resolution completes without panicking on the cycle.
        let result = resolve_peers(&tree, ResolvePeersOptions::default());

        // Every resolved package surfaces a graph entry, even though
        // their peers form a cycle. The exact peer-suffix shape is
        // sensitive to walk order; the important invariant is that
        // every depPath starts with the expected pkg id.
        let dep_paths: Vec<String> =
            result.graph.keys().map(|dp| dp.as_str().to_string()).collect();
        for (name, _) in &[("foo", ""), ("bar", ""), ("qar", ""), ("zoo", "")] {
            let prefix = format!("{name}@1.0.0");
            assert!(
                dep_paths.iter().any(|dp| dp.starts_with(&prefix)),
                "no graph entry starts with {prefix}: {dep_paths:?}",
            );
        }
    }

    /// A package referenced twice in the tree where the peer can
    /// resolve via one parent chain but not the other: both
    /// occurrences must land in the graph — one with the resolved
    /// peer suffix, one without. Ports upstream's `'when a package
    /// is referenced twice in the dependencies graph and one of the
    /// times it cannot resolve its peers, still try to resolve it in
    /// the other occurrence'` (installing/deps-resolver/test/resolvePeers.ts:128).
    ///
    /// Shape:
    /// - root → `zoo` (zoo has child `foo`; foo peers on `qar` — no qar in scope → missing)
    /// - root → `bar` → `zoo` → `foo` (foo peers on `qar`; qar is at bar's level → resolves)
    ///
    /// Expected graph keys (any order):
    /// - `zoo@1.0.0` (the direct-from-root occurrence is pure here —
    ///   its only child `foo` has a missing peer that doesn't
    ///   propagate a peer-suffix back up to zoo)
    /// - `foo@1.0.0` (under root→zoo: missing peer qar)
    /// - `bar@1.0.0`, `qar@1.0.0`
    /// - `zoo@1.0.0(qar@1.0.0)` (under root→bar→zoo: foo's qar peer
    ///   resolves against bar's qar sibling and bubbles through zoo)
    /// - `foo@1.0.0(qar@1.0.0)` (under root→bar→zoo→foo: resolved)
    #[tokio::test]
    async fn revisit_resolves_peer_in_one_occurrence_misses_in_other() {
        let mut table = HashMap::new();
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

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());

        let dep_paths: std::collections::HashSet<String> =
            result.graph.keys().map(|dp| dp.as_str().to_string()).collect();

        // Both `foo` occurrences must surface — one pure (missing
        // peer), one with `(qar@1.0.0)` suffix.
        assert!(
            dep_paths.contains("foo@1.0.0"),
            "missing-peer occurrence of foo missing from graph: {dep_paths:?}",
        );
        assert!(
            dep_paths.contains("foo@1.0.0(qar@1.0.0)"),
            "resolved-peer occurrence of foo missing from graph: {dep_paths:?}",
        );

        // The other occurrence-pairs upstream's test asserts.
        assert!(dep_paths.contains("bar@1.0.0"), "{dep_paths:?}");
        assert!(dep_paths.contains("qar@1.0.0"), "{dep_paths:?}");
        assert!(
            dep_paths.contains("zoo@1.0.0"),
            "direct zoo (no peer suffix) missing: {dep_paths:?}",
        );
        assert!(
            dep_paths.contains("zoo@1.0.0(qar@1.0.0)"),
            "transitive zoo (qar peer bubbled up) missing: {dep_paths:?}",
        );

        // The missing-peer occurrence reports the issue.
        assert!(
            result.peer_dependency_issues.missing.contains_key("qar"),
            "expected missing qar peer issue, got {:?}",
            result.peer_dependency_issues.missing.keys().collect::<Vec<_>>(),
        );
    }

    /// Peer dependencies declared via npm aliases must resolve
    /// against the aliased package by its **real** name. Ports
    /// upstream's `'resolve peer dependencies with npm aliases'`
    /// (installing/deps-resolver/test/resolvePeers.ts:573).
    ///
    /// Importer declares two parallel pairs:
    /// - regular: `foo@1` (peer bar@1), `bar@1`
    /// - aliased: `foo-next: npm:foo@2` (peer bar@2), `bar-next: npm:bar@2`
    ///
    /// The two `foo` versions must each pick the right `bar` peer —
    /// foo@1 against bar@1, foo@2 against bar@2.
    ///
    /// `npm:` aliasing isn't fully ported through the pacquet
    /// `StubResolver` chain yet, so this test is stubbed against a
    /// simpler shape: two distinct package names where each depends
    /// on a distinct peer, exercising the same `parentPkgs` lookup
    /// path the alias case relies on. The TODO captures the gap.
    // TODO(pacquet#?): replace with the real `npm:foo@2` alias once
    // `parse_bare_specifier` routes npm-alias specifiers through the
    // stub resolver in tests.
    #[tokio::test]
    async fn two_peer_chains_resolve_against_their_own_sibling() {
        let mut table = HashMap::new();
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
            fake_result(
                "bar-a",
                "1.0.0",
                serde_json::json!({ "name": "bar-a", "version": "1.0.0" }),
            ),
        );
        table.insert(
            ("bar-b".to_string(), "1.0.0".to_string()),
            fake_result(
                "bar-b",
                "1.0.0",
                serde_json::json!({ "name": "bar-b", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({
            "foo-a": "1.0.0", "bar-a": "1.0.0",
            "foo-b": "1.0.0", "bar-b": "1.0.0",
        }));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());

        // Each foo picks its own bar — they don't cross-pollinate.
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

    /// When a peer requirement isn't satisfied at the importer level
    /// but IS satisfied by a sibling within the parent's children,
    /// the issue list records the "resolved from" parent. Ports
    /// upstream's `'unmet peer dependency issue resolved from
    /// subdependency'` describe-block (installing/deps-resolver/test/resolvePeers.ts:502).
    ///
    /// Shape: root → `foo` → { `dep@1`, `bar` (peers `dep@10`) }.
    /// Importer has no `dep`, so `bar`'s peer requirement is unmet.
    /// The picked `dep@1` (inside foo) violates `bar`'s `^10` range,
    /// so this surfaces as a *bad* peer issue. The `resolvedFrom`
    /// chain must point at `foo` (the parent that *did* supply `dep`,
    /// just at the wrong version).
    #[tokio::test]
    async fn bad_peer_inside_subtree_records_resolved_from_parent() {
        let mut table = HashMap::new();
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

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&tree, ResolvePeersOptions::default());

        // `dep` shows up as a BAD peer (1.0.0 supplied but ^10
        // wanted). No missing entry — the peer WAS resolved, just to
        // the wrong version.
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
}

mod patched_dependencies {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use pacquet_package_manifest::DependencyGroup;
    use pacquet_patching::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord};
    use pacquet_resolving_resolver_base::ResolveOptions;
    use pretty_assertions::assert_eq;

    use super::{StubResolver, fake_manifest, fake_result};
    use crate::resolve_dependency_tree::{
        ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
    };
    use crate::resolve_peers::{ResolvePeersOptions, resolve_peers};
    use pacquet_deps_path::DepPath;

    fn exact_group(version: &str, key: &str, hash: &str) -> PatchGroup {
        let info = ExtendedPatchInfo {
            hash: hash.to_string(),
            patch_file_path: None,
            key: key.to_string(),
        };
        let mut group = PatchGroup::default();
        group.exact.insert(version.to_string(), info);
        group
    }

    /// Resolved-package id gets `(patch_hash=…)` appended for an exact-
    /// version `patchedDependencies` match, the patch key is recorded as
    /// applied, and the depPath the install layer reads carries the
    /// patch suffix.
    #[tokio::test]
    async fn appends_patch_hash_to_pkg_id_and_records_applied_key() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        let mut groups: PatchGroupRecord = PatchGroupRecord::new();
        groups.insert("foo".to_string(), exact_group("1.0.0", "foo@1.0.0", "abc123"));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: Some(Arc::new(groups)),
            },
        )
        .await
        .unwrap();

        assert_eq!(tree.direct.len(), 1);
        assert_eq!(tree.direct[0].id, "foo@1.0.0(patch_hash=abc123)");
        assert!(tree.packages.contains_key("foo@1.0.0(patch_hash=abc123)"));
        assert!(tree.applied_patches.contains("foo@1.0.0"));

        let result = resolve_peers(&tree, ResolvePeersOptions::default());
        assert_eq!(
            result.direct_dependencies_by_alias.get("foo"),
            Some(&DepPath::from("foo@1.0.0(patch_hash=abc123)".to_string())),
        );
    }

    /// Range entries match via `semver.satisfies` and contribute their
    /// configured key to `applied_patches`.
    #[tokio::test]
    async fn range_match_applies_patch_and_records_user_key() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        let info = ExtendedPatchInfo {
            hash: "deadbeef".to_string(),
            patch_file_path: None,
            key: "foo@^1.0.0".to_string(),
        };
        let mut group = PatchGroup::default();
        group.range.push(PatchGroupRangeItem { version: "^1.0.0".to_string(), patch: info });
        let mut groups: PatchGroupRecord = PatchGroupRecord::new();
        groups.insert("foo".to_string(), group);

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: Some(Arc::new(groups)),
            },
        )
        .await
        .unwrap();

        assert_eq!(tree.direct[0].id, "foo@1.2.0(patch_hash=deadbeef)");
        assert!(tree.applied_patches.contains("foo@^1.0.0"));
    }

    /// Configured patches that match no resolved package leave
    /// `applied_patches` empty and the ids unchanged — the
    /// `ERR_PNPM_UNUSED_PATCH` check downstream picks the absence up.
    #[tokio::test]
    async fn unused_patch_leaves_ids_and_applied_set_alone() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        let mut groups: PatchGroupRecord = PatchGroupRecord::new();
        groups.insert("bar".to_string(), exact_group("2.0.0", "bar@2.0.0", "abc"));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: Some(Arc::new(groups)),
            },
        )
        .await
        .unwrap();

        assert_eq!(tree.direct[0].id, "foo@1.0.0");
        assert!(tree.applied_patches.is_empty());
    }

    /// Two ranges that both satisfy the resolved version surface
    /// `ERR_PNPM_PATCH_KEY_CONFLICT` rather than picking arbitrarily.
    #[tokio::test]
    async fn ambiguous_range_match_fails_with_patch_key_conflict() {
        let mut table = HashMap::new();
        table.insert(
            ("foo".to_string(), "^1.0.0".to_string()),
            fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

        let mut group = PatchGroup::default();
        group.range.push(PatchGroupRangeItem {
            version: "^1.0.0".to_string(),
            patch: ExtendedPatchInfo {
                hash: "aaa".to_string(),
                patch_file_path: None,
                key: "foo@^1.0.0".to_string(),
            },
        });
        group.range.push(PatchGroupRangeItem {
            version: "~1.2.0".to_string(),
            patch: ExtendedPatchInfo {
                hash: "bbb".to_string(),
                patch_file_path: None,
                key: "foo@~1.2.0".to_string(),
            },
        });
        let mut groups: PatchGroupRecord = PatchGroupRecord::new();
        groups.insert("foo".to_string(), group);

        let err = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: Some(Arc::new(groups)),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ResolveDependencyTreeError::PatchKeyConflict(_)), "got: {err:?}");
    }
}

mod optional_propagation {
    use super::{
        DependencyGroup, HashMap, Mutex, PackageManifest, ResolveDependencyTreeOptions,
        ResolveOptions, StubResolver, fake_result, resolve_dependency_tree,
    };

    /// `package.json` builder that takes both `dependencies` and
    /// `optionalDependencies` blocks — the bundled `fake_manifest`
    /// helper only writes to `dependencies` so it can't exercise the
    /// importer-level optional flag.
    fn manifest_with_groups(
        prod: serde_json::Value,
        optional: serde_json::Value,
    ) -> (tempfile::TempDir, PackageManifest) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("package.json");
        let json = serde_json::json!({
            "name": "root",
            "version": "0.0.0",
            "dependencies": prod,
            "optionalDependencies": optional,
        });
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
        let manifest = PackageManifest::from_path(path).expect("parse package.json");
        (tmp, manifest)
    }

    /// A direct dep declared under `optionalDependencies` lands on the
    /// resolved package with `optional: true`. Its sibling under
    /// `dependencies` stays `optional: false`. Mirrors upstream's
    /// `getResolvedPackage({ optional: currentIsOptional })` seed on
    /// the first visit.
    #[tokio::test]
    async fn direct_optional_dep_seeds_resolved_package_optional_true() {
        let mut table = HashMap::new();
        table.insert(
            ("opt".to_string(), "^1.0.0".to_string()),
            fake_result("opt", "1.0.0", serde_json::json!({ "name": "opt", "version": "1.0.0" })),
        );
        table.insert(
            ("regular".to_string(), "^1.0.0".to_string()),
            fake_result(
                "regular",
                "1.0.0",
                serde_json::json!({ "name": "regular", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = manifest_with_groups(
            serde_json::json!({ "regular": "^1.0.0" }),
            serde_json::json!({ "opt": "^1.0.0" }),
        );

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod, DependencyGroup::Optional],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        assert!(
            tree.packages.get("opt@1.0.0").expect("opt resolved").optional,
            "direct optionalDependencies entry marks the resolved package optional",
        );
        assert!(
            !tree.packages.get("regular@1.0.0").expect("regular resolved").optional,
            "direct dependencies entry stays optional: false",
        );
    }

    /// A transitive dep reached only through an `optionalDependencies`
    /// edge inherits the flag: `current_is_optional` propagates down
    /// the recursion (`wanted.optional || parent.optional`) so every
    /// descendant of an optional root carries `optional: true`.
    #[tokio::test]
    async fn transitive_dep_under_optional_inherits_optional_true() {
        let mut table = HashMap::new();
        table.insert(
            ("opt".to_string(), "^1.0.0".to_string()),
            fake_result(
                "opt",
                "1.0.0",
                serde_json::json!({
                    "name": "opt",
                    "version": "1.0.0",
                    "dependencies": { "transitive": "^1.0.0" }
                }),
            ),
        );
        table.insert(
            ("transitive".to_string(), "^1.0.0".to_string()),
            fake_result(
                "transitive",
                "1.0.0",
                serde_json::json!({ "name": "transitive", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) =
            manifest_with_groups(serde_json::json!({}), serde_json::json!({ "opt": "^1.0.0" }));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod, DependencyGroup::Optional],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        assert!(
            tree.packages.get("transitive@1.0.0").expect("transitive resolved").optional,
            "child of an optional-only parent inherits optional: true",
        );
    }

    /// A package reachable from BOTH a non-optional and an optional
    /// path AND-folds back to `optional: false`. Mirrors upstream's
    /// `resolvedPkgsById[id].optional = resolvedPkgsById[id].optional && currentIsOptional`
    /// arm on every subsequent visit — a single non-optional path wins.
    #[tokio::test]
    async fn shared_dep_via_non_optional_and_optional_paths_keeps_optional_false() {
        let mut table = HashMap::new();
        table.insert(
            ("opt".to_string(), "^1.0.0".to_string()),
            fake_result(
                "opt",
                "1.0.0",
                serde_json::json!({
                    "name": "opt",
                    "version": "1.0.0",
                    "dependencies": { "shared": "^1.0.0" }
                }),
            ),
        );
        table.insert(
            ("regular".to_string(), "^1.0.0".to_string()),
            fake_result(
                "regular",
                "1.0.0",
                serde_json::json!({
                    "name": "regular",
                    "version": "1.0.0",
                    "dependencies": { "shared": "^1.0.0" }
                }),
            ),
        );
        table.insert(
            ("shared".to_string(), "^1.0.0".to_string()),
            fake_result(
                "shared",
                "1.0.0",
                serde_json::json!({ "name": "shared", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = manifest_with_groups(
            serde_json::json!({ "regular": "^1.0.0" }),
            serde_json::json!({ "opt": "^1.0.0" }),
        );

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod, DependencyGroup::Optional],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        let shared = tree.packages.get("shared@1.0.0").expect("shared resolved");
        assert!(
            !shared.optional,
            "AND-fold: a non-optional path through any consumer wins over an optional one",
        );
    }

    /// A package's own `optionalDependencies` child inherits the
    /// transitive optional flag: an edge marked optional on the
    /// parent's manifest contributes `true` to the child's
    /// `current_is_optional` regardless of how the parent itself was
    /// reached.
    #[tokio::test]
    async fn manifest_level_optional_dependencies_edge_propagates_to_child() {
        let mut table = HashMap::new();
        table.insert(
            ("regular".to_string(), "^1.0.0".to_string()),
            fake_result(
                "regular",
                "1.0.0",
                serde_json::json!({
                    "name": "regular",
                    "version": "1.0.0",
                    "optionalDependencies": { "transitive": "^1.0.0" }
                }),
            ),
        );
        table.insert(
            ("transitive".to_string(), "^1.0.0".to_string()),
            fake_result(
                "transitive",
                "1.0.0",
                serde_json::json!({ "name": "transitive", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) =
            manifest_with_groups(serde_json::json!({ "regular": "^1.0.0" }), serde_json::json!({}));

        let tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod, DependencyGroup::Optional],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
            },
        )
        .await
        .unwrap();

        assert!(
            !tree.packages.get("regular@1.0.0").expect("regular resolved").optional,
            "regular dep stays non-optional",
        );
        assert!(
            tree.packages.get("transitive@1.0.0").expect("transitive resolved").optional,
            "child reached only via a parent's optionalDependencies edge is optional",
        );
    }
}
