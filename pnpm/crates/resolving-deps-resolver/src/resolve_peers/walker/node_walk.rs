use super::{
    AncestorIds, Arc, BTreeMap, CacheHitContext, ChildChains, ChildEdge, ChildOutputs,
    ChildParentRefs, ChildrenWalk, DeferredChildren, DepPath, HashMap, MissingSummary, NodeEntry,
    NodeId, NodeOutput, NodePeersContext, NodeWalkContext, ParentPkgInfo, ParentRefs,
    PeersCacheItem, SettledPeers, SharedChain, WalkResult, WalkedNode, Walker, chain_with_pkg_id,
    merge_realize_undo, pkg_name_version,
};

impl Walker<'_> {}

impl Walker<'_> {
    pub(in super::super) fn resolve_node(
        &mut self,
        node_id: &NodeId,
        walk: &NodeWalkContext<'_>,
    ) -> NodeOutput {
        if let Some(output) =
            self.enter_node(node_id, walk.parent_refs, walk.chain_names, walk.parent_pkg_ids)
        {
            return output;
        }
        let mut entry = self.enter_package(node_id);
        let refs = self.node_child_parent_refs(node_id, &entry, walk);
        let parent_dep_paths = self.record_child_parent_context(
            &refs.refs,
            &refs.own,
            refs.changed,
            walk.parent_dep_paths,
        );

        // `peersCache` lookup. When an earlier walk of this same
        // `pkgIdWithPatchHash` produced a result whose resolved-peer
        // map and missing-peer set are compatible with the current
        // parent peer context, reuse the cached `depPath` and external
        // peer/missing maps without recursing.
        //
        // The cache lookup uses the augmented view because a node's
        // own children count as parents for its own descendants' peer
        // resolution.
        if let Some(cached) =
            self.find_hit(&refs.refs, &entry.pkg.id).map(PeersCacheItem::to_cached_node_output)
        {
            return self.finish_cache_hit(
                cached,
                CacheHitContext {
                    node_id,
                    tree_node_depth: entry.depth,
                    parent_chain_names: walk.chain_names,
                    parent_pkg_ids_chain: walk.parent_pkg_ids,
                    preview_undo: entry.preview_undo,
                },
            );
        }
        let mut walked =
            self.walk_children(node_id, &mut entry, &refs.refs, &parent_dep_paths, walk);
        let settled = self.settle_peers(&entry, &refs.refs, walk, &mut walked);
        self.record_node(node_id, &entry, walk, &mut walked, &settled);

        let output = settled.node_output(&mut walked);
        if self.discovery {
            self.undo_realize(node_id, walked.realize_undo, Some(&output));
        }
        output
    }

    pub(super) fn node_child_parent_refs(
        &self,
        node_id: &NodeId,
        entry: &NodeEntry,
        walk: &NodeWalkContext<'_>,
    ) -> ChildParentRefs {
        self.build_child_parent_refs(
            node_id,
            &entry.pkg,
            walk.parent_refs,
            &entry.provider_children,
            walk.parent_node_ids,
        )
    }

    pub(super) fn enter_package(&mut self, node_id: &NodeId) -> NodeEntry {
        let (pkg_id, depth, installable) = {
            let tree_node = &self.tree.dependencies_tree[node_id];
            (
                Arc::<str>::clone(&tree_node.resolved_package_id),
                tree_node.depth,
                tree_node.installable,
            )
        };
        let pkg = self.owned_package(&pkg_id);
        let (provider_children, preview_undo) = self.preview_peer_provider_children(node_id);
        let (pkg_name, _pkg_version) = pkg_name_version(&pkg.result);
        NodeEntry { pkg, pkg_name, depth, installable, provider_children, preview_undo }
    }

    /// Recurse into children first (post-order). Discovery walks lazy
    /// children directly so cache hits never need occurrence-tree nodes.
    pub(super) fn walk_children(
        &mut self,
        node_id: &NodeId,
        entry: &mut NodeEntry,
        child_parent_refs: &Arc<ParentRefs>,
        parent_dep_paths: &Arc<HashMap<String, ParentPkgInfo>>,
        walk: &NodeWalkContext<'_>,
    ) -> ChildrenWalk {
        let discovery_children = self.discovery_children(node_id, &entry.pkg.id);
        let (children_map, realize_undo) = if discovery_children.is_some() {
            (Arc::new(BTreeMap::new()), None)
        } else {
            self.realize_children_with(node_id, Some(&entry.provider_children))
        };
        let realize_undo = merge_realize_undo(entry.preview_undo.take(), realize_undo);
        let chains = ChildChains {
            names: walk.chain_names.pushed(entry.pkg_name.clone()),
            node_ids: walk.parent_node_ids.pushed(node_id.clone()),
            pkg_ids: chain_with_pkg_id(walk.parent_pkg_ids, &entry.pkg.id),
        };
        let outputs = self.resolve_children_of(
            entry,
            discovery_children.as_ref(),
            &children_map,
            &chains.context(child_parent_refs, parent_dep_paths),
        );
        ChildrenWalk { outputs, children_map, discovery_children, realize_undo, chains }
    }

    pub(super) fn resolve_children_of(
        &mut self,
        entry: &NodeEntry,
        discovery_children: Option<&(Arc<Vec<ChildEdge>>, AncestorIds)>,
        children_map: &BTreeMap<String, NodeId>,
        child_walk: &NodeWalkContext<'_>,
    ) -> ChildOutputs {
        match discovery_children {
            Some((children, parent_ids)) => self.resolve_deferred_children(
                DeferredChildren {
                    pkg_id: &entry.pkg.id,
                    children,
                    parent_ids,
                    provider_children: &entry.provider_children,
                    depth: entry.depth,
                },
                child_walk,
            ),
            None => self.resolve_realized_children(&entry.pkg.id, children_map, child_walk),
        }
    }

    pub(super) fn settle_peers(
        &mut self,
        entry: &NodeEntry,
        child_parent_refs: &Arc<ParentRefs>,
        walk: &NodeWalkContext<'_>,
        walked: &mut ChildrenWalk,
    ) -> SettledPeers {
        let peers = self.resolve_node_peers(NodePeersContext {
            pkg: &entry.pkg,
            pkg_name: &entry.pkg_name,
            parent_refs: child_parent_refs,
            chain_names: &walked.chains.names,
            ancestor_pkg_ids: walk.parent_pkg_ids,
            external_from_children: std::mem::take(&mut walked.outputs.external_peers),
            missing_from_children: &walked.outputs.missing_peers,
        });
        walked.outputs.auto_install_resolved_peers.extend(
            peers
                .own_resolved
                .iter()
                .map(|(peer_name, peer_node_id)| (peer_name.clone(), peer_node_id.clone())),
        );
        let own_missing = (!walked.outputs.missing_peers.is_empty()).then(|| {
            (entry.pkg.id.to_string(), walked.outputs.missing_peers.keys().cloned().collect())
        });
        let subtree_missing_by_pkg = match (own_missing, walked.outputs.missing_summaries.len()) {
            (None, 0) => None,
            (None, 1) => walked.outputs.missing_summaries.pop(),
            (own, _) => Some(Arc::new(MissingSummary {
                own,
                children: std::mem::take(&mut walked.outputs.missing_summaries),
            })),
        };
        SettledPeers {
            is_pure: peers.all_resolved.is_empty() && peers.all_missing.is_empty(),
            dep_path: peers.dep_path,
            own_resolved: peers.own_resolved,
            all_resolved: Arc::new(peers.all_resolved),
            all_missing: Arc::new(peers.all_missing),
            missing_from_children: Arc::new(std::mem::take(&mut walked.outputs.missing_peers)),
            subtree_missing_by_pkg,
        }
    }

    /// Register the depPath ↔ `NodeId` mapping and per-node propagated
    /// state before inserting into the graph (so any cycle the graph
    /// insert hits via the child dep paths can find this node's
    /// depPath).
    pub(super) fn record_node(
        &mut self,
        node_id: &NodeId,
        entry: &NodeEntry,
        walk: &NodeWalkContext<'_>,
        walked: &mut ChildrenWalk,
        settled: &SettledPeers,
    ) {
        self.remember_resolved_node(node_id, &settled.dep_path);
        self.record_walk_result(
            node_id,
            &entry.pkg.id,
            &WalkResult {
                dep_path: &settled.dep_path,
                all_resolved_peers: &settled.all_resolved,
                all_missing_peers: &settled.all_missing,
                missing_peers_of_children: &settled.missing_from_children,
                subtree_missing_by_pkg: &settled.subtree_missing_by_pkg,
                is_pure: settled.is_pure,
            },
        );
        if !self.discovery {
            self.record_walked_node(WalkedNode {
                node_id,
                pkg: &entry.pkg,
                dep_path: &settled.dep_path,
                parent_node_ids: walk.parent_node_ids,
                parent_pkg_ids_chain: walk.parent_pkg_ids,
                children: &walked.children_map,
                child_dep_paths: std::mem::take(&mut walked.outputs.dep_paths),
                all_resolved_peers: &settled.all_resolved,
                all_missing_peers: &settled.all_missing,
                own_resolved_peers: &settled.own_resolved,
                depth: entry.depth,
                installable: entry.installable,
                is_pure: settled.is_pure,
            });
        }
        self.in_progress.remove(node_id);
    }

    /// The node's output when a fast path answers it without a walk, marking
    /// it in progress when there is no such answer and the walk has to run.
    pub(super) fn enter_node(
        &mut self,
        node_id: &NodeId,
        parent_refs: &ParentRefs,
        parent_chain_names: &SharedChain<String>,
        parent_pkg_ids_chain: &SharedChain<String>,
    ) -> Option<NodeOutput> {
        if let Some((tree_node_depth, dep_path)) = self.context_free_dep_path(node_id) {
            self.remember_resolved_node(node_id, &dep_path);
            if let Some(node) = self.graph.get_mut(&dep_path)
                && node.depth > tree_node_depth
            {
                node.depth = tree_node_depth;
            }
            return Some(self.peerless_output(dep_path));
        }

        if self.in_progress.contains(node_id) {
            // Cycle: bottom out with the bare `pkgIdWithPatchHash` as
            // the depPath. The original visit (still on the stack) will
            // compute the real depPath and insert it into
            // `node_dep_paths`. Returning the bare id here ensures the
            // current ancestor's peer-suffix construction can use a
            // `name@version` PeerId — see [`build_peer_id`] for the
            // cycle handling.
            let tree_node = &self.tree.dependencies_tree[node_id];
            let pkg_id = Arc::<str>::clone(&self.tree.packages[&tree_node.resolved_package_id].id);
            return Some(self.peerless_output(DepPath::from(pkg_id)));
        }
        self.in_progress.insert(node_id.clone());

        let cached = {
            let tree_node = &self.tree.dependencies_tree[node_id];
            tree_node
                .has_no_locked_peer_context()
                .then(|| self.find_fast_hit(node_id, parent_refs, &tree_node.resolved_package_id))
                .flatten()
                .map(PeersCacheItem::to_cached_node_output)
        }?;
        let tree_node_depth = self.tree.dependencies_tree[node_id].depth;
        Some(self.finish_cache_hit(
            cached,
            CacheHitContext {
                node_id,
                tree_node_depth,
                parent_chain_names,
                parent_pkg_ids_chain,
                preview_undo: None,
            },
        ))
    }

    /// The depPath a node takes regardless of its parent context, with the
    /// depth it was reached at.
    ///
    /// `purePkgs` fast-path: when the subtree below this
    /// `pkgIdWithPatchHash` resolved with zero external peers and zero
    /// missing peers on a previous walk, AND this package itself declares no
    /// `peerDependencies`, the depPath is the bare `pkgIdWithPatchHash` and
    /// the recursion can be skipped entirely.
    pub(super) fn context_free_dep_path(&self, node_id: &NodeId) -> Option<(i32, DepPath)> {
        let tree_node = self.tree.dependencies_tree.get(node_id)?;
        if tree_node.depth == -1 {
            return Some((
                tree_node.depth,
                DepPath::from(Arc::<str>::clone(&tree_node.resolved_package_id)),
            ));
        }
        let dep_path = self.pure_pkgs.get(&*tree_node.resolved_package_id)?;
        let own_peers_bind =
            !self.tree.packages[&tree_node.resolved_package_id].peer_dependencies.is_empty();
        if own_peers_bind
            || (!self.discovery
                && self
                    .graph
                    .get(dep_path)
                    .is_none_or(|graph_node| graph_node.depth > tree_node.depth))
        {
            return None;
        }
        Some((tree_node.depth, dep_path.clone()))
    }

    /// A node output that contributes nothing to its ancestors' peer
    /// resolution.
    pub(in super::super) fn peerless_output(&self, dep_path: DepPath) -> NodeOutput {
        NodeOutput {
            dep_path,
            external_resolved_peers: Arc::clone(&self.empty_resolved_peers),
            auto_install_resolved_peers: HashMap::default(),
            missing_peers: Arc::clone(&self.empty_missing_peers),
            subtree_missing_by_pkg: None,
        }
    }

    /// Record this node's parent context for the descendants'
    /// [`Walker::peers_cache`] lookups, and hand the snapshot back for the
    /// recursion. It is stored before recursing so a cycle re-entry on a
    /// child also has access to its caller's parent context. Unchanged refs
    /// reuse the caller's snapshot instead of rebuilding an identical map.
    pub(super) fn record_child_parent_context(
        &mut self,
        child_parent_refs: &ParentRefs,
        new_parent_refs: &ParentRefs,
        refs_changed: bool,
        parent_dep_paths: &Arc<HashMap<String, ParentPkgInfo>>,
    ) -> Arc<HashMap<String, ParentPkgInfo>> {
        let parent_dep_paths = if refs_changed {
            self.parent_dep_paths_from_refs(child_parent_refs)
        } else {
            Arc::clone(parent_dep_paths)
        };
        for child_node_id in
            new_parent_refs.values().filter_map(|parent_ref| parent_ref.node_id.as_ref())
        {
            self.parent_pkgs_of_node.insert(child_node_id.clone(), Arc::clone(&parent_dep_paths));
        }
        parent_dep_paths
    }
}
