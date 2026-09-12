//! Peer-resolution context helpers: the propagating parent-reference
//! map, the shared ancestor chains threaded down the walk, `link:`
//! node-id remapping, peer-suffix parsing, and range matching.

pub(super) use peer_specifier::{
    ComparablePeerRange, importer_relative_link_dep_path, link_node_id_as_dep_path, peer_id_pair,
    peer_segment_names, pkg_name, pkg_name_version, remap_link_node_id, satisfies_with_prereleases,
};

mod peer_specifier;
use peer_specifier::version_gte;

use crate::{
    node_id::NodeId,
    resolve_peers::{ResolvePeersOptions, walker::Walker},
    resolved_tree::{ResolvedPackage, TreeChildren},
};
use node_semver::{Range, Version};
use pnpm_deps_path::{DepPath, PeerId, index_of_dep_path_suffix};
use pnpm_resolving_resolver_base::{ResolveResult, get_peer_version_range};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Per-name entry in the propagating [`ParentRefs`] map.
#[derive(Debug, Clone)]
pub(super) struct ParentRef {
    pub(super) version: String,
    /// `None` for top-level deps that were already installed. Pacquet
    /// doesn't surface those yet — `None` only appears on the
    /// importer-level cycle-break fallback, where the `name@version`
    /// form of the peer-id is the only useful representation.
    pub(super) node_id: Option<NodeId>,
    /// Local install name in `node_modules`. May differ from the
    /// package's real name for npm-alias entries.
    pub(super) alias: Option<String>,
    /// Depth at which this parent was added. Threaded into
    /// [`ParentPkgInfo`] so [`Walker::parent_packages_match`] can
    /// apply the depth-equality fallback when peer dependencies are
    /// shadowed across occurrences.
    pub(super) depth: i32,
    /// Per-name shadowing counter. Incremented when a same-name
    /// parent is added at a deeper walk that doesn't match the
    /// existing entry. Used by [`Walker::parent_packages_match`] to
    /// detect shadowed peers.
    pub(super) occurrence: u32,
}

/// `name → ParentRef` map propagated down the walk. Entries are indexed
/// by both the package's real name and its alias when the two differ —
/// `react-dom@npm:next` resolves a `peerDependencies.react-dom`
/// requirement against the alias and `peerDependencies.next` against
/// the real name.
pub(super) type ParentRefs = HashMap<String, ParentRef>;

/// Per-peer-name snapshot stored on [`Walker::parent_pkgs_of_node`].
///
/// `pkg_id` is `None` for parents that came in without a real
/// `NodeId` (the importer-level `topParents` path); those
/// fall back to a pure `version` comparison.
#[derive(Debug, Clone)]
pub(super) struct ParentPkgInfo {
    pub(super) pkg_id: Option<Arc<str>>,
    pub(super) version: Option<String>,
    pub(super) depth: i32,
    pub(super) occurrence: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct SharedChain<Element>(Option<Arc<SharedChainLink<Element>>>);

#[derive(Debug)]
struct SharedChainLink<Element> {
    value: Element,
    parent: Option<Arc<SharedChainLink<Element>>>,
}

impl<Element> Default for SharedChain<Element> {
    fn default() -> Self {
        SharedChain(None)
    }
}

impl<Element> SharedChain<Element> {
    pub(crate) fn pushed(&self, value: Element) -> Self {
        SharedChain(Some(Arc::new(SharedChainLink { value, parent: self.0.clone() })))
    }

    pub(crate) fn iter(&self) -> SharedChainIter<'_, Element> {
        SharedChainIter { next: self.0.as_deref() }
    }
}

impl<Element: PartialEq> SharedChain<Element> {
    /// Takes `&str` rather than `&Element` so a caller holding a
    /// shared package id can test membership without allocating a
    /// `String` to compare against.
    pub(super) fn contains_str(&self, value: &str) -> bool
    where
        Element: AsRef<str>,
    {
        self.iter().any(|item| item.as_ref() == value)
    }
}

/// Memo for [`SharedChain::any_memoized`], keyed by link address: the
/// answer for a link covers that link and everything above it, so it
/// holds for every chain that shares the suffix. One memo is valid for
/// one predicate.
///
/// Each entry keeps the link it is keyed on alive. Addresses are only
/// unique among live allocations, so a dropped link could otherwise
/// hand its address — and its answer — to whatever was allocated there
/// next.
pub(super) struct ChainSuffixMemo<Element> {
    answers: HashMap<usize, (Arc<SharedChainLink<Element>>, bool)>,
}

impl<Element> Default for ChainSuffixMemo<Element> {
    fn default() -> Self {
        ChainSuffixMemo { answers: HashMap::default() }
    }
}

impl<Element> SharedChain<Element> {
    /// Whether any element from here to the root satisfies `predicate`,
    /// answering from `memo` for suffixes already evaluated. Chains built
    /// by pushing onto a common ancestor share those suffixes, so a
    /// repeated query over a family of chains costs each link once
    /// instead of once per chain.
    ///
    /// `predicate` must be pure: it is called on the links this call is
    /// first to reach, in root-to-tip order, and skipped entirely for
    /// suffixes the memo already answers.
    pub(super) fn any_memoized(
        &self,
        memo: &mut ChainSuffixMemo<Element>,
        mut predicate: impl FnMut(&Element) -> bool,
    ) -> bool {
        let mut unmemoized = Vec::new();
        let mut cursor = self.0.as_ref();
        let mut satisfied = false;
        while let Some(link) = cursor {
            if let Some(&(_, answer)) = memo.answers.get(&link_key(link)) {
                satisfied = answer;
                break;
            }
            unmemoized.push(link);
            cursor = link.parent.as_ref();
        }
        for link in unmemoized.into_iter().rev() {
            satisfied = satisfied || predicate(&link.value);
            memo.answers.insert(link_key(link), (Arc::clone(link), satisfied));
        }
        satisfied
    }
}

fn link_key<Element>(link: &Arc<SharedChainLink<Element>>) -> usize {
    Arc::as_ptr(link) as usize
}

impl<Element: Clone> SharedChain<Element> {
    pub(crate) fn to_root_vec(&self) -> Vec<Element> {
        let mut values: Vec<Element> = self.iter().cloned().collect();
        values.reverse();
        values
    }
}

pub(crate) struct SharedChainIter<'a, Element> {
    next: Option<&'a SharedChainLink<Element>>,
}

impl<'a, Element> Iterator for SharedChainIter<'a, Element> {
    type Item = &'a Element;

    fn next(&mut self) -> Option<Self::Item> {
        let link = self.next?;
        self.next = link.parent.as_deref();
        Some(&link.value)
    }
}

/// One importer whose direct dependencies count as "current" peer
/// providers for the must-win guard of locked-peer-provider reuse.
pub(super) struct CurrentProviderSource {
    pub(super) direct_node_ids_by_alias: HashMap<String, NodeId>,
    pub(super) declared_direct_dependencies: HashSet<String>,
    pub(super) explicitly_requested_direct_dependencies: HashSet<String>,
}

impl Walker<'_> {
    /// Build the `(peer_name → ParentPkgInfo)` snapshot that gets
    /// stored on [`Self::parent_pkgs_of_node`] for each child the
    /// caller is about to descend into.
    ///
    /// `link:` parents don't have a real tree entry; pacquet's
    /// [`ParentRef`] keeps the `NodeId` but the tree-lookup falls back
    /// to a pure `version` comparison.
    pub(super) fn parent_dep_paths_from_refs(
        &self,
        parent_refs: &ParentRefs,
    ) -> Arc<HashMap<String, ParentPkgInfo>> {
        let mut out = HashMap::default();
        for (name, parent_ref) in parent_refs {
            if !self.tree.all_peer_dep_names.contains(name) {
                continue;
            }
            let pkg_id = parent_ref
                .node_id
                .as_ref()
                .and_then(|nid| self.tree.dependencies_tree.get(nid))
                .map(|tn| std::sync::Arc::<str>::clone(&tn.resolved_package_id));
            let version = pkg_id.is_none().then(|| parent_ref.version.clone());
            out.insert(
                name.clone(),
                ParentPkgInfo {
                    pkg_id,
                    version,
                    depth: parent_ref.depth,
                    occurrence: parent_ref.occurrence,
                },
            );
        }
        // Shared, not cloned: the same snapshot is recorded for every
        // child of a node, and these maps dominated the walker's
        // allocation churn when cloned per child.
        Arc::new(out)
    }

    pub(super) fn parent_refs_match(&self, current: &ParentRef, new: &ParentRef) -> bool {
        if current.version != new.version || current.alias != new.alias {
            return false;
        }
        let Some(current_name) = self.parent_ref_package_name(current) else {
            return true;
        };
        let Some(new_name) = self.parent_ref_package_name(new) else {
            return true;
        };
        current_name == new_name
    }

    fn parent_ref_package_name(&self, parent_ref: &ParentRef) -> Option<String> {
        let node_id = parent_ref.node_id.as_ref()?;
        let tree_node = self.tree.dependencies_tree.get(node_id)?;
        let pkg = self.tree.packages.get(&tree_node.resolved_package_id)?;
        Some(pkg_name_version(&pkg.result).0)
    }

    pub(super) fn inherited_parent_pkg_breaks_peer_diamond(
        &self,
        parent_refs: &ParentRefs,
        inherited_parent_pkg: &ParentRef,
        own_child_parent_pkg: &ParentRef,
        node_id: &NodeId,
    ) -> bool {
        let (Some(inherited_node_id), Some(own_child_node_id)) =
            (inherited_parent_pkg.node_id.as_ref(), own_child_parent_pkg.node_id.as_ref())
        else {
            return false;
        };
        if inherited_node_id == own_child_node_id {
            return false;
        }
        let Some(inherited_context) = self.parent_pkgs_of_node.get(inherited_node_id) else {
            return false;
        };
        let Some(parent_pkg) = self
            .tree
            .dependencies_tree
            .get(own_child_node_id)
            .and_then(|node| self.tree.packages.get(&node.resolved_package_id))
        else {
            return false;
        };
        let (parent_pkg_name, _) = pkg_name_version(&parent_pkg.result);

        let conflicting_peers =
            self.conflicting_peer_names(parent_refs, inherited_context, parent_pkg);
        if conflicting_peers.is_empty() {
            return false;
        }
        self.child_binds_conflicting_peer(node_id, &parent_pkg_name, &conflicting_peers)
    }

    /// The peers `parent_pkg` declares that the inherited provider's own
    /// context and the current one disagree about.
    fn conflicting_peer_names(
        &self,
        parent_refs: &ParentRefs,
        inherited_context: &HashMap<String, ParentPkgInfo>,
        parent_pkg: &ResolvedPackage,
    ) -> HashSet<String> {
        let mut conflicting_peers = HashSet::default();
        for peer_name in parent_pkg.peer_dependencies.keys() {
            if !self.tree.all_peer_dep_names.contains(peer_name) {
                continue;
            }
            let Some(inherited_peer) = inherited_context.get(peer_name) else { continue };
            let Some(current_peer) = parent_refs.get(peer_name) else { continue };
            if self.parent_peer_differs(current_peer, inherited_peer) {
                conflicting_peers.insert(peer_name.clone());
            }
        }
        conflicting_peers
    }

    /// Whether a child of `node_id` closes the diamond: it takes the provider
    /// as a peer *and* takes one of the peers the two contexts disagree
    /// about.
    fn child_binds_conflicting_peer(
        &self,
        node_id: &NodeId,
        parent_pkg_name: &str,
        conflicting_peers: &HashSet<String>,
    ) -> bool {
        let Some(node) = self.tree.dependencies_tree.get(node_id) else { return false };
        for child_pkg_id in self.child_pkg_ids_of(node) {
            let Some(child_pkg) = self.tree.packages.get(child_pkg_id) else { continue };
            if !child_pkg.peer_dependencies.contains_key(parent_pkg_name) {
                continue;
            }
            if conflicting_peers.iter().any(|peer| child_pkg.peer_dependencies.contains_key(peer)) {
                return true;
            }
        }
        false
    }

    /// The package ids of a node's children, from its realized map or, while
    /// it is still lazy, from the tree's per-package child edges.
    fn child_pkg_ids_of(&self, node: &crate::DependenciesTreeNode) -> Vec<&str> {
        match &node.children {
            TreeChildren::Realized(children) => children
                .values()
                .filter_map(|child_node_id| self.tree.dependencies_tree.get(child_node_id))
                .map(|child| &*child.resolved_package_id)
                .collect(),
            TreeChildren::Lazy { .. } => {
                let recorded = self
                    .tree
                    .children_by_id
                    .get(&node.resolved_package_id)
                    .into_iter()
                    .flat_map(|children| children.iter());
                recorded.map(|child| &*child.pkg_id).collect()
            }
        }
    }

    fn parent_peer_differs(
        &self,
        current_peer: &ParentRef,
        inherited_peer: &ParentPkgInfo,
    ) -> bool {
        if let Some(inherited_pkg_id) = inherited_peer.pkg_id.as_ref() {
            let Some(current_node_id) = current_peer.node_id.as_ref() else {
                return true;
            };
            return self
                .tree
                .dependencies_tree
                .get(current_node_id)
                .is_none_or(|node| *node.resolved_package_id != **inherited_pkg_id);
        }
        inherited_peer.version.as_ref() != Some(&current_peer.version)
    }
}

/// Record a parent ref under both keys: each parent is recorded by its
/// install alias *and* its real name when the two differ.
///
/// `parent_node_id` is the [`NodeId`] the parent should appear under
/// in the [`ParentRefs`] map. For most parents this is just
/// `direct.node_id`, but `link:` parents may carry the remapped
/// node id produced by [`remap_link_node_id`] when
/// `excludeLinksFromLockfile` is on.
pub(super) fn insert_parent_ref(
    refs: &mut ParentRefs,
    direct_alias: &str,
    parent_node_id: NodeId,
    pkg: &ResolvedPackage,
    depth: i32,
) {
    let (real_name, version) = pkg_name_version(&pkg.result);
    let parent_ref = ParentRef {
        version,
        node_id: Some(parent_node_id),
        alias: (direct_alias != real_name).then(|| direct_alias.to_string()),
        depth,
        occurrence: 0,
    };
    update_parent_refs(refs, direct_alias, &parent_ref);
    if direct_alias != real_name {
        update_parent_refs(refs, &real_name, &parent_ref);
    }
}

fn update_parent_refs(refs: &mut ParentRefs, new_alias: &str, parent_ref: &ParentRef) {
    if let Some(existing) = refs.get(new_alias) {
        let existing_has_alias = existing.alias.as_deref().is_some_and(|alias| alias != new_alias);
        if !existing_has_alias {
            return;
        }
        let new_has_alias = parent_ref.alias.as_deref().is_some_and(|alias| alias != new_alias);
        if new_has_alias && version_gte(&existing.version, &parent_ref.version) {
            return;
        }
    }
    refs.insert(new_alias.to_string(), parent_ref.clone());
}

#[cfg(test)]
mod tests;
