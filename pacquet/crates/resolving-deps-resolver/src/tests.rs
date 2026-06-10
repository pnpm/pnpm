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

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
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
            manifest_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
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
    assert_eq!(foo_tree_node.children.realized().len(), 1);
    let bar_node_id = foo_tree_node.children.realized().get("bar").unwrap();
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
            manifest_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
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
    let shared_via_a = a_tree.children.realized().get("shared").unwrap();
    let shared_via_b = b_tree.children.realized().get("shared").unwrap();
    assert_eq!(shared_via_a, shared_via_b);
    let shared_occurrences =
        tree.dependencies_tree.values().filter(|n| n.resolved_package_id == "shared@1.0.0").count();
    assert_eq!(shared_occurrences, 1);
}

/// Regression for [#11939](https://github.com/pnpm/pnpm/issues/11939):
/// a workspace-link dependency whose linked package declares peer
/// dependencies (the babylon `@dev/shared-ui-components` shape) must
/// short-circuit in the tree builder. Mirrors upstream's
/// [`isLinkedDependency` arm](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/deps-resolver/src/resolveDependencies.ts#L926-L937):
///
/// 1. The tree node carries `depth = -1`.
/// 2. The tree node's `children` map is empty — the link target
///    resolves its own deps as a separate importer, not as nested
///    transitive deps of the parent.
/// 3. The package's `peer_dependencies` is empty — peer matching is
///    the linked importer's responsibility.
///
/// Together these ensure the peer-resolution stage's `depth == -1`
/// short-circuit kicks in and the link node's depPath stays
/// `link:<rel-path>` with no peer-graph suffix.
#[tokio::test]
async fn workspace_link_node_is_short_circuited_in_tree() {
    use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
    use pacquet_resolving_resolver_base::PkgResolutionId;

    let link_id = "link:../shared";
    let mut table = HashMap::new();
    table.insert(
        ("shared".to_string(), "workspace:*".to_string()),
        ResolveResult {
            id: PkgResolutionId::from(link_id.to_string()),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(std::sync::Arc::new(serde_json::json!({
                "name": "shared",
                "version": "1.0.0",
                // The linked package itself carries peers — these
                // must NOT propagate to the parent's tree because
                // pnpm's `isLinkedDependency` branch sets
                // `resolvedPackage: { name, version }` only.
                "peerDependencies": { "react": "^18.0.0" },
                "dependencies": { "lodash": "^4.0.0" },
            }))),
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "../shared".to_string(),
            }),
            resolved_via: "workspace".to_string(),
            normalized_bare_specifier: None,
            alias: Some("shared".to_string()),
            policy_violation: None,
        },
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "shared": "workspace:*" }));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
        },
    )
    .await
    .expect("resolve tree");

    assert_eq!(tree.direct.len(), 1);
    let link_node_id = &tree.direct[0].node_id;
    let link_node = tree.dependencies_tree.get(link_node_id).expect("link tree node");
    assert_eq!(link_node.depth, -1, "link node must carry depth = -1");
    assert!(
        link_node.children.realized().is_empty(),
        "link node must have empty children — link target resolves its own deps separately",
    );
    let pkg = tree.packages.get(link_id).expect("link package entry");
    assert!(
        pkg.peer_dependencies.is_empty(),
        "link node's ResolvedPackage must carry no peer_dependencies — peer matching is the linked importer's responsibility",
    );
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
            manifest_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
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

/// A transitive dependency whose alias contains `..` segments would
/// escape the `node_modules` directory when joined onto a modules
/// path. The walker rejects it before any further resolution work.
/// Mirrors the pnpm-side fix in
/// `installing/deps-resolver/src/validateDependencyAlias.ts`.
#[tokio::test]
async fn transitive_dep_with_traversal_alias_is_rejected() {
    let mut table = HashMap::new();
    table.insert(
        ("normal".to_string(), "1.0.0".to_string()),
        fake_result(
            "normal",
            "1.0.0",
            serde_json::json!({
                "name": "normal",
                "version": "1.0.0",
                "dependencies": { "@x/../../../../../.git/hooks": "1.0.0" },
            }),
        ),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "normal": "1.0.0" }));

    let err = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
        },
    )
    .await
    .expect_err("traversal alias must error");
    match err {
        ResolveDependencyTreeError::InvalidDependencyName { parent, alias } => {
            assert_eq!(alias, "@x/../../../../../.git/hooks");
            assert!(
                parent.contains("normal"),
                "parent must name the offending package, got {parent:?}",
            );
        }
        other => panic!("expected InvalidDependencyName, got {other:?}"),
    }
}

mod block_exotic_subdeps {
    use std::{collections::HashMap, sync::Mutex};

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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .expect("exotic subdep must pass when disabled");
        assert!(tree.packages.contains_key("say-hi@1.0.0"));
    }
}

mod peers {
    use std::{collections::HashMap, sync::Mutex};

    use pacquet_package_manifest::DependencyGroup;
    use pacquet_resolving_resolver_base::ResolveOptions;
    use pretty_assertions::assert_eq;

    use super::{StubResolver, fake_manifest, fake_result};
    use crate::{
        resolve_dependency_tree::{ResolveDependencyTreeOptions, resolve_dependency_tree},
        resolve_peers::{ResolvePeersOptions, resolve_peers},
    };
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
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
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
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
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
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
        // Peer-suffix uses the resolved (17.0.0) version — the install
        // proceeds with the picked candidate.
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
    /// Ports upstream's
    /// [`'uses version-only peer suffixes without nested dep paths'`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/test/resolvePeers.ts#L679-L756).
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
                "@emotion/styled@11.0.0(@emotion/react@11.0.0(react@18.0.0))(react@18.0.0)"
                    .to_string(),
                "react@18.0.0".to_string(),
            ],
        );
    }

    /// Transitive peer: `a` depends on `b`, `b` has peer `c`, importer
    /// has direct `a` + `c`. Even though `a` has no peers itself, its
    /// child `b` carries `c` as an external peer, so `a` propagates `c`
    /// up to its own depPath suffix too. Both `a` and `b` land as
    /// `…(c@1.0.0)`. Mirrors upstream's
    /// [`'transitive peers use version-only suffixes'`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/test/resolvePeers.ts#L758-L833).
    ///
    /// Pacquet's [`DepPath`] uses `name@version` for pure packages, so
    /// the `dedupe_peers=true` vs `false` rendering of a pure peer is
    /// byte-identical (both produce `(c@1.0.0)`). Upstream's pnpm uses
    /// `c/1.0.0` for the dep-path form and so observes a difference;
    /// the contract this test locks down is the transitive-peer
    /// propagation itself, not the byte shape of the peer-id.
    #[tokio::test]
    async fn dedupe_peers_propagates_transitive_peer_to_parent() {
        let mut table = HashMap::new();
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
                pnpmfile_hook: None,
                read_package_log: None,
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
            vec![
                "a@1.0.0(c@1.0.0)".to_string(),
                "b@1.0.0(c@1.0.0)".to_string(),
                "c@1.0.0".to_string(),
            ],
        );
    }

    /// A package's graph-node `depth` is the minimum across all
    /// occurrences, even when the shallower one short-circuits through the
    /// `pure_pkgs` fast path (which has no `NodeRecord`). `p` is reached at
    /// depth 2 via `a → b → p` (walked first, so its record carries depth
    /// 2) and at depth 1 via `c → p` (a pure-pkgs revisit). The rebuilt
    /// graph must record depth 1. Regression guard for the `build_final_graph`
    /// depth tie-break.
    #[tokio::test]
    async fn shallower_pure_pkgs_revisit_lowers_graph_depth() {
        let mut table = HashMap::new();
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
                    "dependencies": { "p": "1.0.0" }
                }),
            ),
        );
        table.insert(
            ("c".to_string(), "1.0.0".to_string()),
            fake_result(
                "c",
                "1.0.0",
                serde_json::json!({
                    "name": "c",
                    "version": "1.0.0",
                    "dependencies": { "p": "1.0.0" }
                }),
            ),
        );
        // `p` has a dep `q` so it gets per-occurrence NodeIds (a shared
        // leaf would already carry its minimum depth in the tree). Its
        // whole subtree is peer-free, so the second, shallower occurrence
        // under `c` takes the `pure_pkgs` fast path — which records no
        // `NodeRecord`, the case the rebuild must still account for.
        table.insert(
            ("p".to_string(), "1.0.0".to_string()),
            fake_result(
                "p",
                "1.0.0",
                serde_json::json!({
                    "name": "p",
                    "version": "1.0.0",
                    "dependencies": { "q": "1.0.0" }
                }),
            ),
        );
        table.insert(
            ("q".to_string(), "1.0.0".to_string()),
            fake_result("q", "1.0.0", serde_json::json!({ "name": "q", "version": "1.0.0" })),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        // `a` precedes `c` so `p` is first walked at depth 2 (`a → b → p`),
        // then revisited at depth 1 (`c → p`).
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "a": "1.0.0", "c": "1.0.0" }));
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
        let p_node = &result.graph[&DepPath::from("p@1.0.0".to_string())];
        assert_eq!(p_node.depth, 1, "p's graph depth must be the minimum (1), not 2");
    }

    /// Port of upstream's
    /// [`"a peer's own peer is shared with a sibling that peer-depends both"`](https://github.com/pnpm/pnpm/blob/894ea6af2c/installing/deps-resolver/test/resolvePeers.ts#L1207).
    /// `plugin` peer-depends both `parser` and `typescript`; `parser`
    /// peer-depends `typescript`. `plugin` and `parser` live under
    /// `umbrella` (under `app`, which also brings `typescript@1.0.0`), while
    /// the importer also has a top-level `typescript@2.0.0` + `parser@1.0.0`.
    /// `plugin`'s `parser` must resolve `typescript@1.0.0` — the version
    /// `plugin` itself uses — not be shadowed by the top-level `parser` that
    /// resolved `typescript@2.0.0`. Exercises the depPath suffix machinery.
    #[tokio::test]
    async fn peers_own_peer_shared_with_sibling_that_peer_depends_both() {
        let mut table = HashMap::new();
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
                pnpmfile_hook: None,
                read_package_log: None,
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
        let mut table = HashMap::new();
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
                pnpmfile_hook: None,
                read_package_log: None,
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

    /// Shared fixture for the `dedupe_peers_*` pair: react@18 plus
    /// `@emotion/react@11` (peer: react) plus `@emotion/styled@11`
    /// (peers: react, @emotion/react). Mirrors upstream's
    /// `dedupePeers` test fixture at the linked commit above.
    async fn resolve_emotion_fixture(
        opts: ResolvePeersOptions,
    ) -> crate::resolve_peers::ResolvePeersResult {
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
            ("@emotion/react".to_string(), "11.0.0".to_string()),
            fake_result(
                "@emotion/react",
                "11.0.0",
                serde_json::json!({
                    "name": "@emotion/react",
                    "version": "11.0.0",
                    "peerDependencies": { "react": ">=16" }
                }),
            ),
        );
        table.insert(
            ("@emotion/styled".to_string(), "11.0.0".to_string()),
            fake_result(
                "@emotion/styled",
                "11.0.0",
                serde_json::json!({
                    "name": "@emotion/styled",
                    "version": "11.0.0",
                    "peerDependencies": {
                        "react": ">=16",
                        "@emotion/react": ">=11"
                    }
                }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({
            "react": "18.0.0",
            "@emotion/react": "11.0.0",
            "@emotion/styled": "11.0.0",
        }));
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();
        resolve_peers(&mut tree, opts)
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
        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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

        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

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

    /// Same package reached via two parent chains where the peer
    /// resolves only via one: both occurrences must land in the
    /// graph with distinct depPaths. Ports upstream's `'when a
    /// package is referenced twice in the dependencies graph and one
    /// of the times it cannot resolve its peers, still try to
    /// resolve it in the other occurrence'`
    /// (installing/deps-resolver/test/resolvePeers.ts:128).
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

        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

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

    /// Two parallel peer chains in one importer — each peer resolves
    /// against its own sibling, no cross-pollination. Stands in for
    /// upstream's `'resolve peer dependencies with npm aliases'`
    /// (installing/deps-resolver/test/resolvePeers.ts:573); the
    /// real alias case needs `npm:` plumbing in the stub resolver.
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

        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

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

    /// A peer satisfied by a wrong-version sibling inside the
    /// parent's subtree surfaces as a *bad* peer (not missing).
    /// Stands in for upstream's `'unmet peer dependency issue
    /// resolved from subdependency'` describe-block
    /// (installing/deps-resolver/test/resolvePeers.ts:502); the
    /// `resolvedFrom` field upstream tracks isn't exposed on
    /// pacquet's `PeerDependencyIssue` yet.
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

        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());

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

    /// A child whose first walk had no manifest (`result.manifest ==
    /// None` — the shape git / tarball / local resolvers return when
    /// the registry response carries no manifest body) must keep the
    /// non-leaf classification the eager walk picked when it's reached
    /// again through a lazy revisit. Regression for the manifest-less
    /// divergence between `pkg_is_leaf` and the inferred
    /// `children_by_id.is_empty()` check the lazy realizer used before
    /// `ResolvedPackage::is_leaf` was persisted.
    #[tokio::test]
    async fn revisit_with_no_manifest_child_keeps_per_occurrence_node_id() {
        use crate::node_id::NodeId;
        let mut table = HashMap::new();
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
                    "dependencies": { "manifestless": "^1.0.0" },
                    "peerDependencies": { "peer": "^1.0.0" }
                }),
            ),
        );
        // `manifestless` resolves without a manifest body. Without
        // persistence, the lazy realizer would misclassify this as a
        // leaf (children_by_id entry is empty AND peer_dependencies
        // is empty) and collapse the revisit onto `NodeId::Leaf`.
        let manifestless = {
            let mut result = fake_result(
                "manifestless",
                "1.0.0",
                serde_json::json!({ "name": "manifestless", "version": "1.0.0" }),
            );
            result.manifest = None;
            result
        };
        table.insert(("manifestless".to_string(), "^1.0.0".to_string()), manifestless);
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
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        // First-walk classification must be non-leaf — `pkg_is_leaf`
        // returns false when the manifest is None.
        assert!(
            !tree.packages.get("manifestless@1.0.0").expect("manifestless resolved").is_leaf,
            "manifest-less packages must keep is_leaf=false (eager walker contract)",
        );

        resolve_peers(&mut tree, ResolvePeersOptions::default());

        // After lazy realization, every occurrence of `manifestless`
        // must use a Counter NodeId — the same shape the eager walker
        // assigned on first visit. A `Leaf` NodeId here would mean
        // `realize_children` misclassified the package and collapsed
        // distinct occurrences, breaking per-call-site state for any
        // future visitor that descends through it.
        let manifestless_node_ids: Vec<&NodeId> = tree
            .dependencies_tree
            .iter()
            .filter(|(_, node)| node.resolved_package_id == "manifestless@1.0.0")
            .map(|(id, _)| id)
            .collect();
        assert!(
            !manifestless_node_ids.is_empty(),
            "expected at least one tree entry for manifestless",
        );
        for id in &manifestless_node_ids {
            assert!(
                matches!(id, NodeId::Counter(_)),
                "manifest-less child must use Counter NodeId in every occurrence, got {id:?}",
            );
        }
    }

    /// A pure package (no peer deps, peer-clean subtree) reached
    /// through multiple parents only realizes its children for the
    /// occurrence the peer resolver walks first. Subsequent
    /// occurrences hit the `purePkgs` short-circuit before
    /// `realize_children` runs, so their [`TreeChildren::Lazy`]
    /// stays Lazy. Regression guard against accidentally moving the
    /// realize call above the short-circuit.
    #[tokio::test]
    async fn pure_revisit_leaves_lazy_children_unrealized() {
        use crate::resolved_tree::TreeChildren;
        let mut table = HashMap::new();
        table.insert(
            ("p1".to_string(), "^1.0.0".to_string()),
            fake_result(
                "p1",
                "1.0.0",
                serde_json::json!({
                    "name": "p1",
                    "version": "1.0.0",
                    "dependencies": { "pure": "^1.0.0" }
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
                    "dependencies": { "pure": "^1.0.0" }
                }),
            ),
        );
        // `pure` has a child so it is non-leaf (per-occurrence
        // NodeId), but no peer deps anywhere in the subtree — that
        // makes it eligible for `purePkgs`.
        table.insert(
            ("pure".to_string(), "^1.0.0".to_string()),
            fake_result(
                "pure",
                "1.0.0",
                serde_json::json!({
                    "name": "pure",
                    "version": "1.0.0",
                    "dependencies": { "pure_leaf": "^1.0.0" }
                }),
            ),
        );
        table.insert(
            ("pure_leaf".to_string(), "^1.0.0".to_string()),
            fake_result(
                "pure_leaf",
                "1.0.0",
                serde_json::json!({ "name": "pure_leaf", "version": "1.0.0" }),
            ),
        );
        let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
        let (_tmp, manifest) = fake_manifest(serde_json::json!({ "p1": "^1.0.0", "p2": "^1.0.0" }));

        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: None,
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        // Sanity: `pure` got two per-occurrence tree entries, the
        // first carrying Realized children (eager walk), the second
        // carrying Lazy children (revisit).
        let pure_pre: Vec<(&crate::node_id::NodeId, bool)> = tree
            .dependencies_tree
            .iter()
            .filter(|(_, node)| node.resolved_package_id == "pure@1.0.0")
            .map(|(id, node)| (id, matches!(node.children, TreeChildren::Lazy { .. })))
            .collect();
        assert_eq!(pure_pre.len(), 2, "expected two occurrences of pure, got {pure_pre:?}");
        assert!(
            pure_pre.iter().any(|(_, is_lazy)| !*is_lazy),
            "first walk should produce a Realized entry",
        );
        assert!(
            pure_pre.iter().any(|(_, is_lazy)| *is_lazy),
            "revisit should produce a Lazy entry",
        );

        resolve_peers(&mut tree, ResolvePeersOptions::default());

        // After peer resolution: the lazy occurrence stays Lazy
        // because `purePkgs` short-circuits before `realize_children`
        // is called.
        let still_lazy = tree
            .dependencies_tree
            .iter()
            .filter(|(_, node)| node.resolved_package_id == "pure@1.0.0")
            .filter(|(_, node)| matches!(node.children, TreeChildren::Lazy { .. }))
            .count();
        assert_eq!(
            still_lazy, 1,
            "purePkgs short-circuit must leave the revisit's lazy children un-realized",
        );
    }

    /// Ported from upstream pnpm's
    /// [`path to external link is not added to the lockfile, when it resolves a peer dependency`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/install/excludeLinksFromLockfile.ts#L224-L243)
    /// e2e test, narrowed to the peer-resolution slice.
    ///
    /// Scenario: a registry package `abc` peer-depends on `peer-a`. The
    /// importer also depends on `peer-a` via a bare `link:` to an
    /// external directory (outside the lockfile root). With
    /// `excludeLinksFromLockfile = true`, the link's parent-ref node
    /// id gets remapped to `link:node_modules/peer-a`, the peer suffix
    /// uses `link_path_to_peer_version("node_modules/peer-a") =
    /// "node_modules+peer-a"`, and the snapshot child edge for the
    /// peer points at the same `link:node_modules/peer-a` instead of
    /// the original absolute path.
    #[tokio::test]
    async fn external_link_peer_remaps_to_node_modules_when_exclude_links_on() {
        use pacquet_lockfile::{DirectoryResolution, LockfileResolution};
        use pacquet_resolving_resolver_base::PkgResolutionId;

        let link_id = "link:/abs/external";
        let mut table = HashMap::new();
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
            pacquet_resolving_resolver_base::ResolveResult {
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
                pnpmfile_hook: None,
                read_package_log: None,
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
        let peer_child =
            abc_node.children.get("peer-a").expect("abc snapshot has a peer-a child edge");
        assert_eq!(
            peer_child,
            &DepPath::from("link:node_modules/peer-a".to_string()),
            "snapshot child ref reuses the remapped link node id verbatim",
        );
    }
}

mod patched_dependencies {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use pacquet_package_manifest::DependencyGroup;
    use pacquet_patching::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord};
    use pacquet_resolving_resolver_base::ResolveOptions;
    use pretty_assertions::assert_eq;

    use super::{StubResolver, fake_manifest, fake_result};
    use crate::{
        resolve_dependency_tree::{
            ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
        },
        resolve_peers::{ResolvePeersOptions, resolve_peers},
    };
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

        let mut tree = resolve_dependency_tree(
            &resolver,
            &manifest,
            [DependencyGroup::Prod],
            ResolveDependencyTreeOptions {
                base_opts: ResolveOptions::default(),
                patched_dependencies: Some(Arc::new(groups)),
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(tree.direct.len(), 1);
        assert_eq!(tree.direct[0].id, "foo@1.0.0(patch_hash=abc123)");
        assert!(tree.packages.contains_key("foo@1.0.0(patch_hash=abc123)"));
        assert!(tree.applied_patches.contains("foo@1.0.0"));

        let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
    #[expect(
        clippy::needless_pass_by_value,
        reason = "test helpers take owned literal fixtures by value to keep call sites clean"
    )]
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
                manifest_hook: None,
                pnpmfile_hook: None,
                read_package_log: None,
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
