//! Tree-building helpers shared by the peer-resolution test modules.

use crate::{
    node_id::NodeId,
    resolve_peers::{
        ResolvePeersOptions,
        discovery::PeerDiscoveryCaches,
        walker::{PeerWalkMode, Walker},
    },
    resolved_tree::{
        ChildEdge, DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedPackageInput,
        ResolvedTree, TreeChildren,
    },
};
use pnpm_lockfile::{
    DirectoryResolution, LockfileResolution, PkgName, PkgNameVer, TarballResolution,
};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::FxHashMap as HashMap;
use std::{collections::BTreeMap, str::FromStr, sync::Arc};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) enum PeerCacheMode {
    #[default]
    Enabled,
    Bypass,
}

pub(super) fn tree_node(
    pkg_id: &str,
    children: BTreeMap<String, NodeId>,
    depth: i32,
) -> DependenciesTreeNode {
    DependenciesTreeNode::new(
        Arc::from(pkg_id.to_string()),
        TreeChildren::Realized(Arc::new(children)),
        depth,
    )
}

/// A regular (non-optional) child edge.
pub(super) fn child_edge(alias: &str, pkg_id: &str) -> ChildEdge {
    ChildEdge { alias: alias.to_string(), pkg_id: Arc::from(pkg_id), optional: false }
}

/// Add an importer-level direct dependency whose children the peer walk
/// expands from [`ResolvedTree::children_by_id`].
pub(super) fn add_lazy_direct_dep(
    dependencies_tree: &mut HashMap<NodeId, DependenciesTreeNode>,
    direct: &mut Vec<DirectDep>,
    alias: &str,
    pkg_id: &str,
) {
    let node_id = NodeId::next();
    dependencies_tree.insert(
        node_id.clone(),
        DependenciesTreeNode::new(Arc::from(pkg_id), TreeChildren::Lazy, 0),
    );
    direct.push(DirectDep { alias: alias.to_string(), node_id, id: pkg_id.into() });
}

pub(super) fn walker_for_tests(tree: &mut ResolvedTree) -> Walker<'_> {
    Walker::new(
        tree,
        ResolvePeersOptions::default(),
        HashMap::default(),
        Vec::new(),
        PeerDiscoveryCaches::default(),
        PeerWalkMode::Final,
    )
}

pub(super) fn package(
    name: &str,
    version: &str,
    peer_dependencies: &[(&str, &str)],
    is_leaf: bool,
) -> ResolvedPackage {
    let peer_dependencies: Vec<_> = peer_dependencies
        .iter()
        .map(|(name, version)| (*name, *version, false))
        .collect();
    package_with_peer_dependencies(name, version, &peer_dependencies, is_leaf)
}

pub(super) fn package_with_peer_dependencies(
    name: &str,
    version: &str,
    peer_dependencies: &[(&str, &str, bool)],
    is_leaf: bool,
) -> ResolvedPackage {
    let peer_dependencies = peer_dependencies
        .iter()
        .map(|(name, version, optional)| {
            ((*name).to_string(), PeerDep { version: (*version).to_string(), optional: *optional })
        })
        .collect();
    ResolvedPackage::new(ResolvedPackageInput {
        id: format!("{name}@{version}").into(),
        result: Arc::new(resolve_result(name, version)),
        peer_set: crate::PeerSet::taking_all(peer_dependencies),
        optional: false,
        is_leaf,
    })
}

pub(super) fn linked_package(name: &str, id: &str, directory: &str) -> ResolvedPackage {
    ResolvedPackage::new(ResolvedPackageInput {
        id: Arc::from(id.to_string()),
        result: Arc::new(ResolveResult {
            id: PkgResolutionId::from(id.to_string()),
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: directory.to_string(),
            }),
            resolved_via: "workspace".to_string(),
            normalized_bare_specifier: None,
            alias: Some(name.to_string()),
            policy_violation: None,
            package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: Some(Arc::new(serde_json::json!({ "name": name, "version": "1.0.0" }))),
                non_deprecated_alternative: None,
            },
        }),
        peer_set: crate::PeerSet::default(),
        optional: false,
        is_leaf: true,
    })
}

pub(super) fn resolve_result(name: &str, version: &str) -> ResolveResult {
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{name}-{version}.tgz"),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
        package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
            name_ver: Some(name_ver),
            latest: Some(version.to_string()),
            published_at: None,
            manifest: None,
            non_deprecated_alternative: None,
        },
    }
}

/// Compare acyclic peer graphs exactly. Cache sharing can change the
/// unrolling of peer cycles, which requires binding assertions instead.
pub(super) fn resolve_with_cache_oracle(
    tree: &mut ResolvedTree,
    importers: &[super::ImporterPeerInput],
    lockfile_dir: &std::path::Path,
    settings: super::WorkspacePeerSettings,
    opts: ResolvePeersOptions,
) -> super::WorkspaceResolvePeersResult {
    let mut full_tree = tree.clone();
    let mut full_opts = opts.clone();
    full_opts.cache_mode = PeerCacheMode::Bypass;
    let cached = super::resolve_peers_workspace(tree, importers, lockfile_dir, settings, opts);
    let full = super::resolve_peers_workspace(
        &mut full_tree,
        importers,
        lockfile_dir,
        settings,
        full_opts,
    );
    assert_eq!(cached.direct_dependencies_by_importer, full.direct_dependencies_by_importer);
    assert_eq!(
        cached.graph.len(),
        full.graph.len(),
        "cached={:?}, full={:?}",
        cached.graph.keys().collect::<Vec<_>>(),
        full.graph.keys().collect::<Vec<_>>(),
    );
    for (path, cached_node) in &cached.graph {
        let full_node = &full.graph[path];
        assert_eq!(cached_node.edges.children, full_node.edges.children, "{path}");
        assert_eq!(
            cached_node.edges.optional_children, full_node.edges.optional_children,
            "{path}",
        );
        assert_eq!(
            cached_node.edges.resolved_peer_names, full_node.edges.resolved_peer_names,
            "{path}",
        );
        assert_eq!(
            cached_node.edges.transitive_peer_dependencies,
            full_node.edges.transitive_peer_dependencies,
            "{path}",
        );
    }
    // Cache hits summarize descendant locations at the reused subtree.
    let missing = |issues: &crate::PeerDependencyIssues| -> std::collections::BTreeSet<_> {
        issues.missing
            .iter()
            .flat_map(|(name, entries)| {
                entries
                    .iter()
                    .map(move |entry| (name.clone(), entry.raw_range.clone(), entry.optional))
            })
            .collect()
    };
    for (importer, cached_issues) in &cached.peer_dependency_issues_by_importer {
        let full_issues = &full.peer_dependency_issues_by_importer[importer];
        assert_eq!(missing(cached_issues), missing(full_issues), "{importer}");
        assert_eq!(cached_issues.bad, full_issues.bad, "{importer}");
    }
    cached
}
