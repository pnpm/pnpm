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
    let id = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id,
        latest: Some(version.to_string()),
        published_at: None,
        manifest: Some(manifest),
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
            auto_install_peers: false,
            base_opts: ResolveOptions::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct.len(), 1);
    assert_eq!(tree.direct[0].alias, "foo");
    assert_eq!(tree.direct[0].id, "foo@1.2.0");
    assert_eq!(tree.packages.len(), 2);
    assert!(tree.packages.contains_key("foo@1.2.0"));
    let foo_node_id = tree.direct[0].node_id;
    let foo_tree_node = tree.dependencies_tree.get(&foo_node_id).unwrap();
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
            auto_install_peers: false,
            base_opts: ResolveOptions::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.packages.len(), 3);
    assert!(tree.packages.contains_key("a@1.0.0"));
    assert!(tree.packages.contains_key("b@1.0.0"));
    assert!(tree.packages.contains_key("shared@1.0.0"));
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
            auto_install_peers: false,
            base_opts: ResolveOptions::default(),
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
                auto_install_peers: false,
                base_opts: ResolveOptions::default(),
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
                auto_install_peers: false,
                base_opts: ResolveOptions::default(),
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
                auto_install_peers: false,
                base_opts: ResolveOptions::default(),
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
                auto_install_peers: false,
                base_opts: ResolveOptions::default(),
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
                auto_install_peers: false,
                base_opts: ResolveOptions::default(),
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
}
