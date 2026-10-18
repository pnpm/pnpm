use super::{
    Arc, CurrentProviderSource, DepPath, HashMap, MissingPeerInfo, NodeId, NodePeers,
    NodePeersContext, ParentRef, ParentRefs, PeerId, ResolvedPackage, SharedChain, TreeChildren,
    Walker, index_of_dep_path_suffix,
};

use crate::peer_provider::{LockedProvider, PeerScope, ProviderPick, provider_for};

impl Walker<'_> {
    /// Resolve this node's own peer requirements against the augmented
    /// [`ParentRefs`] visible at it, fold the result with what its
    /// children reported, and render the depPath. Empty resolved-peers
    /// ⇒ pure node: depPath = `pkgIdWithPatchHash`.
    pub(super) fn resolve_node_peers(&mut self, context: NodePeersContext<'_>) -> NodePeers {
        let mut own_resolved: HashMap<String, NodeId> = HashMap::default();
        let mut own_missing: HashMap<String, MissingPeerInfo> = HashMap::default();
        for (peer_name, peer_dep) in context.pkg.peer_set.taken() {
            self.resolve_one_peer(
                peer_name,
                peer_dep,
                context.parent_refs,
                context.chain_names,
                context.ancestor_pkg_ids,
                &mut own_resolved,
                &mut own_missing,
            );
        }

        // A package doesn't peer-depend on itself, so its own name never
        // enters its suffix.
        let mut all_resolved = context.external_from_children;
        for (peer_alias, peer_node_id) in &own_resolved {
            all_resolved.insert(peer_alias.clone(), peer_node_id.clone());
        }
        all_resolved.remove(context.pkg_name);

        let mut all_missing = context.missing_from_children.clone();
        for (peer_alias, info) in &own_missing {
            all_missing.insert(peer_alias.clone(), info.clone());
        }

        let dep_path = self.peer_dep_path(&context.pkg.id, &all_resolved);
        NodePeers { own_resolved, all_resolved, all_missing, dep_path }
    }

    /// Empty resolved peers ⇒ pure node: depPath = `pkgIdWithPatchHash`.
    pub(super) fn peer_dep_path(
        &self,
        pkg_id: &Arc<str>,
        all_resolved: &HashMap<String, NodeId>,
    ) -> DepPath {
        if all_resolved.is_empty() {
            return DepPath::from(Arc::<str>::clone(pkg_id));
        }
        let peer_ids: Vec<PeerId> = all_resolved
            .iter()
            .map(|(peer_alias, peer_node_id)| self.build_peer_id(peer_alias, peer_node_id))
            .collect();
        let suffix =
            pnpm_lockfile::PeerBindings::suffix_of(&peer_ids, self.opts.peers_suffix_max_length);
        DepPath::from(format!("{pkg_id}{suffix}"))
    }

    /// The pins the wanted lockfile's peer context puts into `parent_refs`
    /// (upstream `resolvePeers.ts:594`): for each peer the node declares,
    /// the recorded provider when [`provider_for`] picks it. The inherited
    /// binding already sits in `parent_refs`, so only the pins are
    /// returned. They are computed without mutating `parent_refs`, so the
    /// caller can keep sharing an unchanged map. Each pin reads only its own
    /// name's current binding, so collecting against the pre-pin map is
    /// equivalent to inserting while iterating.
    pub(super) fn locked_peer_context_pins(
        &self,
        node_id: &NodeId,
        pkg: &ResolvedPackage,
        parent_refs: &ParentRefs,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> Vec<(String, ParentRef)> {
        let mut pins = Vec::new();
        let (Some(locked_peer_context), Some(provider_paths)) = (
            self.tree.dependencies_tree
                .get(node_id)
                .and_then(crate::resolved_tree::DependenciesTreeNode::locked_peer_context),
            self.opts.scope.resolved_peer_provider_paths.as_ref(),
        ) else {
            return pins;
        };
        let site = PinSite { pkg, provider_paths, parent_refs, parent_node_ids };
        pins.extend(
            locked_peer_context
                .iter()
                .filter_map(|(peer_name, previous_dep_path)| {
                    self.locked_peer_pin(peer_name, previous_dep_path, &site)
                }),
        );
        pins
    }

    /// The pin one entry of the wanted lockfile's peer context puts into the
    /// scope, when [`provider_for`] picks the recorded provider.
    fn locked_peer_pin(
        &self,
        peer_name: &str,
        previous_dep_path: &DepPath,
        site: &PinSite<'_>,
    ) -> Option<(String, ParentRef)> {
        let peer_dep = site.pkg.peer_set.taken().get(peer_name)?;
        let locked = self.locked_provider(previous_dep_path, site.provider_paths)?;
        let scope = PeerScope {
            inherited: None,
            inherited_must_win: || {
                self.has_current_peer_provider_that_must_win(
                    peer_name,
                    site.parent_refs,
                    site.parent_node_ids,
                )
            },
            locked: Some(locked),
        };
        let ProviderPick::Bound {
            provider: (peer_node_id, depth, version),
            pinned_by_lockfile: true,
        } = provider_for(&peer_dep.version, scope)
        else {
            return None;
        };
        Some((
            peer_name.to_string(),
            ParentRef {
                version: Arc::clone(version),
                node_id: Some(peer_node_id.clone()),
                alias: Some(peer_name.to_string()),
                depth,
                occurrence: 0,
            },
        ))
    }

    /// The provider the wanted lockfile recorded at `previous_dep_path`, as
    /// its node, depth, and version, or `None` when this pass resolved it elsewhere or
    /// to a path with peers of its own, which depends on the bindings this
    /// pass is still computing.
    pub(super) fn locked_provider(
        &self,
        previous_dep_path: &DepPath,
        provider_paths: &HashMap<NodeId, DepPath>,
    ) -> Option<LockedProvider<'_, (&NodeId, i32, &Arc<str>)>> {
        let peer_node_id = self.providers.node_ids_by_previous_dep_path.get(previous_dep_path)?;
        let unchanged = provider_paths.get(peer_node_id) == Some(previous_dep_path)
            && index_of_dep_path_suffix(previous_dep_path.as_str()).peers_index.is_none()
            && self.caches.node_dep_paths
                .get(peer_node_id)
                .is_none_or(|current| current == previous_dep_path);
        if !unchanged {
            return None;
        }
        let peer_tree_node = self.tree.dependencies_tree.get(peer_node_id)?;
        let peer_pkg = self.tree.packages.get(&peer_tree_node.resolved_package_id)?;
        Some(LockedProvider {
            provider: (peer_node_id, peer_tree_node.depth, peer_pkg.version()),
            version: peer_pkg.version(),
        })
    }

    /// The upstream `hasCurrentPeerProviderThatMustWin`: the current
    /// provider bound for `peer_name` wins over a locked one when it is
    /// an importer direct dep under a *different* alias, one the user
    /// explicitly requested, one the manifest declares with no
    /// wanted-lockfile resolution, or a child an ancestor re-resolved
    /// away from the lockfile.
    pub(super) fn has_current_peer_provider_that_must_win(
        &self,
        peer_name: &str,
        parent_refs: &ParentRefs,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> bool {
        let Some(peer_node_id) =
            parent_refs.get(peer_name).and_then(|parent| parent.node_id.as_ref())
        else {
            return false;
        };
        self.direct_dep_provider_must_win(peer_name, peer_node_id)
            || self.ancestor_reresolved_provider(peer_node_id, parent_node_ids)
    }

    pub(super) fn direct_dep_provider_must_win(
        &self,
        peer_name: &str,
        peer_node_id: &NodeId,
    ) -> bool {
        self.providers.current_provider_sources
            .iter()
            .any(|source| {
                source.direct_node_ids_by_alias
                    .iter()
                    .any(|(alias, direct_node_id)| {
                        direct_node_id == peer_node_id
                            && self.direct_alias_must_win(source, alias, peer_name, peer_node_id)
                    })
            })
    }

    pub(super) fn direct_alias_must_win(
        &self,
        source: &CurrentProviderSource,
        alias: &str,
        peer_name: &str,
        peer_node_id: &NodeId,
    ) -> bool {
        alias != peer_name
            || source.explicitly_requested_direct_dependencies.contains(alias)
            || (source.declared_direct_dependencies.contains(alias)
                && self.tree.dependencies_tree
                    .get(peer_node_id)
                    .is_none_or(|node| node.previous_dep_path().is_none()))
    }

    /// Whether an ancestor on the walk path re-resolved this provider away
    /// from the lockfile, which its own children must see.
    pub(super) fn ancestor_reresolved_provider(
        &self,
        peer_node_id: &NodeId,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> bool {
        for parent_node_id in parent_node_ids.iter() {
            let Some(parent_node) = self.tree.dependencies_tree.get(parent_node_id) else {
                continue;
            };
            let Some(must_win) = parent_node.must_win_dependency_names() else {
                continue;
            };
            // Ancestors on the walk path always have realized children.
            let TreeChildren::Realized(children) = &parent_node.children else { continue };
            if must_win
                .iter()
                .any(|alias| children.get(alias) == Some(peer_node_id))
            {
                return true;
            }
        }
        false
    }
}

/// What one node's locked peer pins are checked against.
struct PinSite<'a> {
    pkg: &'a ResolvedPackage,
    provider_paths: &'a HashMap<NodeId, DepPath>,
    parent_refs: &'a ParentRefs,
    parent_node_ids: &'a SharedChain<NodeId>,
}
