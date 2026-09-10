use super::{
    DependencyGroup, HashMap, Mutex, ResolveDependencyTreeOptions, ResolveOptions, ResolveResult,
    StubResolver, assert_eq, fake_manifest, resolve_dependency_tree,
};

/// Regression for [#11939](https://github.com/pnpm/pnpm/issues/11939):
/// a workspace-link dependency whose linked package declares peer
/// dependencies (the babylon `@dev/shared-ui-components` shape) must
/// short-circuit in the tree builder.
///
/// The short-circuit is what makes the peer-resolution stage's
/// `depth == -1` arm kick in so the link node's depPath stays
/// `link:<rel-path>` with no peer-graph suffix.
#[tokio::test]
async fn workspace_link_node_is_short_circuited_in_tree() {
    use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
    use pnpm_resolving_resolver_base::PkgResolutionId;

    let link_id = "link:../shared";
    let mut table = HashMap::default();
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
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
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
