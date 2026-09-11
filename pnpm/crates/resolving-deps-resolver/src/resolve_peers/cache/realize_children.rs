use super::{
    Arc, BTreeMap, ChildEdge, DeferredChildContext, DeferredChildResolution, DependenciesTreeNode,
    EdgeRealization, HashSet, LazyProviders, NodeId, NodeOutput, ParentRefs, PeersCacheItem,
    SharedChain, TreeChildren, UndoRealize, Walker, should_retain_materialized_node,
};

impl Walker<'_> {
    pub(super) fn deferred_child_resolution(
        &self,
        parent_refs: &ParentRefs,
        pkg_id: &Arc<str>,
    ) -> DeferredChildResolution {
        if let Some(dep_path) = self.pure_pkgs.get(&**pkg_id)
            && self.tree.packages[&**pkg_id].peer_dependencies.is_empty()
        {
            return DeferredChildResolution::Pure(dep_path.clone());
        }
        if let Some(cached) = self
            .find_fast_hit_for_lazy(parent_refs, pkg_id)
            .map(PeersCacheItem::to_cached_node_output)
            && cached.output.missing_peers.is_empty()
        {
            return DeferredChildResolution::Cached(cached);
        }
        DeferredChildResolution::Materialize(Arc::<str>::clone(pkg_id))
    }

    pub(in super::super) fn resolve_deferred_child(
        &mut self,
        context: &DeferredChildContext<'_>,
    ) -> NodeOutput {
        match self.deferred_child_resolution(context.walk.parent_refs, &context.edge.pkg_id) {
            DeferredChildResolution::Pure(dep_path) => self.peerless_output(dep_path),
            DeferredChildResolution::Cached(cached) => cached.output,
            DeferredChildResolution::Materialize(pkg_id) => {
                self.tree.dependencies_tree.insert(
                    context.node_id.clone(),
                    DependenciesTreeNode::new(
                        pkg_id,
                        TreeChildren::Lazy { parent_ids: context.parent_ids.clone() },
                        context.depth,
                        true,
                    ),
                );
                let output = self.resolve_node(&context.node_id, context.walk);
                if !self.parent_pkgs_of_node.contains_key(&context.node_id)
                    && !should_retain_materialized_node(
                        &self.retained_peer_node_ids,
                        Some(&output),
                        &context.node_id,
                    )
                {
                    self.tree.dependencies_tree.remove(&context.node_id);
                    self.node_dep_paths.remove(&context.node_id);
                    self.visited_this_call.remove(&context.node_id);
                }
                output
            }
        }
    }

    pub(in super::super) fn previously_resolved_children(
        &mut self,
        parent_node_ids: &SharedChain<NodeId>,
        parent_pkg_ids_chain: &SharedChain<String>,
        current_pkg_id: &str,
    ) -> BTreeMap<String, NodeId> {
        let mut children = BTreeMap::new();
        if !parent_pkg_ids_chain.iter().any(|pkg_id| pkg_id == current_pkg_id) {
            return children;
        }
        for parent_node_id in parent_node_ids.iter() {
            let same_pkg = self
                .tree
                .dependencies_tree
                .get(parent_node_id)
                .is_some_and(|node| &*node.resolved_package_id == current_pkg_id);
            if !same_pkg {
                continue;
            }
            for (alias, child_node_id) in self.realize_children(parent_node_id).0.iter() {
                children.entry(alias.clone()).or_insert_with(|| child_node_id.clone());
            }
        }
        children
    }

    pub(in super::super) fn optional_child_aliases(
        &self,
        pkg_id: &str,
        edges: &BTreeMap<String, NodeId>,
    ) -> HashSet<String> {
        self.tree
            .children_by_id
            .get(pkg_id)
            .into_iter()
            .flat_map(|children| children.iter())
            .filter(|edge| edge.optional && edges.contains_key(&edge.alias))
            .map(|edge| edge.alias.clone())
            .collect()
    }

    pub(in super::super) fn preview_peer_provider_children(
        &mut self,
        node_id: &NodeId,
    ) -> (BTreeMap<String, NodeId>, Option<UndoRealize>) {
        let node = &self.tree.dependencies_tree[node_id];
        match &node.children {
            TreeChildren::Realized(children) => (self.realized_provider_children(children), None),
            TreeChildren::Lazy { parent_ids } => self.lazy_provider_children(LazyProviders {
                parent_ids: parent_ids.clone(),
                pkg_id: std::sync::Arc::<str>::clone(&node.resolved_package_id),
                depth: node.depth,
            }),
        }
    }

    pub(super) fn realized_provider_children(
        &self,
        children: &BTreeMap<String, NodeId>,
    ) -> BTreeMap<String, NodeId> {
        children
            .iter()
            .filter(|(alias, child_node_id)| {
                self.tree
                    .dependencies_tree
                    .get(*child_node_id)
                    .and_then(|child| self.tree.packages.get(&child.resolved_package_id))
                    .is_some_and(|pkg| self.is_peer_relevant(alias, pkg))
            })
            .map(|(alias, child_node_id)| (alias.clone(), child_node_id.clone()))
            .collect()
    }

    /// Insert a lazy node's peer-relevant children as lazy nodes of
    /// their own, so the providers can be previewed without realizing
    /// the whole children map.
    pub(super) fn lazy_provider_children(
        &mut self,
        lazy: LazyProviders,
    ) -> (BTreeMap<String, NodeId>, Option<UndoRealize>) {
        let children = self.tree.children_by_id.get(&lazy.pkg_id).cloned().unwrap_or_default();
        let provider_edge_indices = self
            .peer_provider_children_by_pkg_id
            .get(&*lazy.pkg_id)
            .map_or(&[][..], |providers| providers.relevant_edge_indices.as_slice());
        let canonical_scc = self.canonical_scc();
        let full_chain = lazy.parent_ids.pushed(lazy.pkg_id.to_string());
        let mut providers = BTreeMap::new();
        let mut newly_inserted = Vec::new();
        for &edge_index in provider_edge_indices {
            let edge = &children[edge_index];
            let Some(pkg) = self.tree.packages.get(&edge.pkg_id) else { continue };
            if Self::cuts_cycle_edge(&canonical_scc, &lazy.pkg_id, &edge.pkg_id) {
                continue;
            }
            let child_node_id =
                if pkg.is_leaf { NodeId::leaf(&edge.pkg_id) } else { NodeId::next() };
            if !self.tree.dependencies_tree.contains_key(&child_node_id) {
                self.tree.dependencies_tree.insert(
                    child_node_id.clone(),
                    DependenciesTreeNode::new(
                        std::sync::Arc::<str>::clone(&edge.pkg_id),
                        TreeChildren::Lazy { parent_ids: full_chain.clone() },
                        lazy.depth + 1,
                        true,
                    ),
                );
                newly_inserted.push(child_node_id.clone());
            }
            providers.insert(edge.alias.clone(), child_node_id);
        }
        (providers, Some(UndoRealize { newly_inserted, prev_parent_ids: lazy.parent_ids }))
    }

    /// Realize the `(alias → NodeId)` children of `node_id` if it's
    /// currently a [`TreeChildren::Lazy`] entry; return the realized
    /// map (cloned for the caller). On a [`TreeChildren::Realized`]
    /// entry, just clones and returns. Expands the thunk on demand:
    ///
    /// 1. Walk [`crate::ResolvedTree::children_by_id`] for this node's
    ///    package id.
    /// 2. Skip any child whose pkg id appears in `parent_ids` — that
    ///    edge would form a cycle.
    /// 3. For each surviving child, allocate a per-occurrence
    ///    `NodeId` (leaves reuse the deterministic `NodeId::leaf`
    ///    for the leaf-collapse the eager walker does too) and
    ///    insert a fresh `dependencies_tree` entry with another
    ///    `Lazy` children variant that carries `parent_ids +
    ///    [self_pkg_id]` for cycle break on its own descendants.
    /// 4. Flip this node's `children` field to `Realized` so a
    ///    later visitor reuses the map.
    pub(super) fn realize_children(
        &mut self,
        node_id: &NodeId,
    ) -> (Arc<BTreeMap<String, NodeId>>, Option<UndoRealize>) {
        self.realize_children_with(node_id, None)
    }

    pub(in super::super) fn realize_children_with(
        &mut self,
        node_id: &NodeId,
        previewed: Option<&BTreeMap<String, NodeId>>,
    ) -> (Arc<BTreeMap<String, NodeId>>, Option<UndoRealize>) {
        // Snapshot the bits we need; we'll mutate `self.tree` below
        // and can't hold a borrow on the entry across the mutation.
        let (parent_ids, pkg_id, depth) = {
            let node = &self.tree.dependencies_tree[node_id];
            match &node.children {
                // Cheap: the realized map is shared, not copied per revisit.
                TreeChildren::Realized(map) => {
                    return (Arc::clone(map), None);
                }
                TreeChildren::Lazy { parent_ids } => {
                    (parent_ids.clone(), Arc::<str>::clone(&node.resolved_package_id), node.depth)
                }
            }
        };
        // No spec means the first walk never recorded children for this
        // package id — defensive empty case.
        let children_spec =
            self.tree.children_by_id.get(&pkg_id).map_or_else(|| Arc::new(Vec::new()), Arc::clone);
        let canonical_scc = self.canonical_scc();
        let context = EdgeRealization {
            canonical_scc: &canonical_scc,
            full_chain: &parent_ids.pushed(pkg_id.to_string()),
            pkg_id: &pkg_id,
            child_depth: depth + 1,
            previewed,
        };
        let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
        let mut newly_inserted: Vec<NodeId> = Vec::new();
        for edge in children_spec.iter() {
            let Some(child_node_id) = self.realize_child_edge(edge, &context, &mut newly_inserted)
            else {
                continue;
            };
            realized.insert(edge.alias.clone(), child_node_id);
        }
        let realized = Arc::new(realized);
        // Replace this node's `Lazy` with `Realized` so future
        // visitors reuse the work.
        if let Some(node) = self.tree.dependencies_tree.get_mut(node_id) {
            node.children = TreeChildren::Realized(Arc::clone(&realized));
        }
        (realized, Some(UndoRealize { newly_inserted, prev_parent_ids: parent_ids }))
    }

    /// The `NodeId` `edge` gets in the realized map, or `None` when the edge
    /// leads nowhere the walk may follow.
    pub(super) fn realize_child_edge(
        &mut self,
        edge: &ChildEdge,
        context: &EdgeRealization<'_>,
        newly_inserted: &mut Vec<NodeId>,
    ) -> Option<NodeId> {
        if Self::cuts_cycle_edge(context.canonical_scc, context.pkg_id, &edge.pkg_id) {
            if *context.pkg_id == edge.pkg_id {
                return None;
            }
            // A canonical back-edge is still a real dependency edge:
            // record it against the target's shared canonical
            // occurrence without giving the walk a path through it.
            return Some(self.canonical_backedge_node(&edge.pkg_id, context.child_depth));
        }
        let child_node_id = self.child_node_id_for_edge(edge, context.previewed);
        if self.ensure_child_node(&child_node_id, edge, context) {
            newly_inserted.push(child_node_id.clone());
        }
        Some(child_node_id)
    }

    /// Reuse the first walk's classification (persisted on
    /// [`crate::resolved_tree::ResolvedPackage::is_leaf`] by `pkg_is_leaf`).
    /// Defaults to non-leaf when the package isn't in `packages` — same shape
    /// as the eager walker's `manifest == None` arm, and `NodeId::next()`
    /// keeps occurrences distinct so a later visit can still observe
    /// per-call-site state.
    pub(in super::super) fn child_node_id_for_edge(
        &self,
        edge: &ChildEdge,
        previewed: Option<&BTreeMap<String, NodeId>>,
    ) -> NodeId {
        if let Some(previewed_node_id) = previewed.and_then(|previewed| previewed.get(&edge.alias))
        {
            return previewed_node_id.clone();
        }
        let is_leaf = self.tree.packages.get(&edge.pkg_id).is_some_and(|pkg| pkg.is_leaf);
        if is_leaf { NodeId::leaf(&edge.pkg_id) } else { NodeId::next() }
    }

    /// Whether a fresh `dependencies_tree` entry had to be created, which the
    /// caller has to undo when the realization is rolled back.
    pub(super) fn ensure_child_node(
        &mut self,
        child_node_id: &NodeId,
        edge: &ChildEdge,
        context: &EdgeRealization<'_>,
    ) -> bool {
        let child_depth = context.child_depth;
        let Some(node) = self.tree.dependencies_tree.get_mut(child_node_id) else {
            self.tree.dependencies_tree.insert(
                child_node_id.clone(),
                DependenciesTreeNode::new(
                    Arc::<str>::clone(&edge.pkg_id),
                    TreeChildren::Lazy { parent_ids: context.full_chain.clone() },
                    child_depth,
                    true,
                ),
            );
            return true;
        };
        if node.depth > child_depth {
            node.depth = child_depth;
        }
        false
    }

    pub(in super::super) fn undo_realize(
        &mut self,
        node_id: &NodeId,
        undo: Option<UndoRealize>,
        output: Option<&NodeOutput>,
    ) {
        let Some(undo) = undo else { return };
        for child_id in &undo.newly_inserted {
            if should_retain_materialized_node(&self.retained_peer_node_ids, output, child_id) {
                continue;
            }
            self.tree.dependencies_tree.remove(child_id);
            self.parent_pkgs_of_node.remove(child_id);
            self.node_dep_paths.remove(child_id);
            self.visited_this_call.remove(child_id);
        }
        if let Some(node) = self.tree.dependencies_tree.get_mut(node_id) {
            node.children = TreeChildren::Lazy { parent_ids: undo.prev_parent_ids };
        }
    }
}
