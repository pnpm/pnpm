//! Tree-building helpers shared by the peer-resolution test modules.

use crate::{
    node_id::NodeId,
    resolve_peers::{ResolvePeersOptions, discovery::PeerDiscoveryCaches, walker::Walker},
    resolved_tree::{DependenciesTreeNode, PeerDep, ResolvedPackage, ResolvedTree, TreeChildren},
};
use pnpm_lockfile::{
    DirectoryResolution, LockfileResolution, PkgName, PkgNameVer, TarballResolution,
};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::FxHashMap as HashMap;
use std::{collections::BTreeMap, str::FromStr, sync::Arc};

pub(super) fn tree_node(
    pkg_id: &str,
    children: BTreeMap<String, NodeId>,
    depth: i32,
) -> DependenciesTreeNode {
    DependenciesTreeNode::new(
        Arc::from(pkg_id.to_string()),
        TreeChildren::Realized(Arc::new(children)),
        depth,
        true,
    )
}

pub(super) fn walker_for_tests(tree: &mut ResolvedTree) -> Walker<'_> {
    Walker::new(
        tree,
        ResolvePeersOptions::default(),
        HashMap::default(),
        Vec::new(),
        PeerDiscoveryCaches::default(),
        false,
    )
}

pub(super) fn package(
    name: &str,
    version: &str,
    peer_dependencies: &[(&str, &str)],
    is_leaf: bool,
) -> ResolvedPackage {
    let peer_dependencies: Vec<_> =
        peer_dependencies.iter().map(|(name, version)| (*name, *version, false)).collect();
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
    ResolvedPackage {
        id: format!("{name}@{version}").into(),
        result: Arc::new(resolve_result(name, version)),
        peer_dependencies,
        optional: false,
        is_leaf,
    }
}

pub(super) fn linked_package(name: &str, id: &str, directory: &str) -> ResolvedPackage {
    ResolvedPackage {
        id: Arc::from(id.to_string()),
        result: Arc::new(ResolveResult {
            id: PkgResolutionId::from(id.to_string()),
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(Arc::new(serde_json::json!({ "name": name, "version": "1.0.0" }))),
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: directory.to_string(),
            }),
            resolved_via: "workspace".to_string(),
            normalized_bare_specifier: None,
            alias: Some(name.to_string()),
            policy_violation: None,
        }),
        peer_dependencies: BTreeMap::new(),
        optional: false,
        is_leaf: true,
    }
}

pub(super) fn resolve_result(name: &str, version: &str) -> ResolveResult {
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: Some(version.to_string()),
        published_at: None,
        manifest: None,
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
    }
}
