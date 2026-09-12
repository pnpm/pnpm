use super::{
    AncestorIds, Arc, BTreeMap, ChildAliases, ChildEdge, ChildOutputs, ChildParentRefs,
    DeferredChildContext, DeferredChildren, NodeId, NodeOutput, NodeWalkContext, ParentRefs,
    PeersCacheItem, ResolvedPackage, SharedChain, TreeChildren, WalkResult, Walker,
    insert_parent_ref,
};

impl Walker<'_> {}

impl Walker<'_> {
    /// The still-lazy children a discovery walk descends into directly, with
    /// the ancestor chain they see. `None` outside discovery, or once the
    /// node's children have been realized.
    pub(super) fn discovery_children(
        &self,
        node_id: &NodeId,
        pkg_id: &Arc<str>,
    ) -> Option<(Arc<Vec<ChildEdge>>, AncestorIds)> {
        if !self.discovery {
            return None;
        }
        let TreeChildren::Lazy { parent_ids } = &self.tree.dependencies_tree[node_id].children
        else {
            return None;
        };
        Some((
            self.tree.children_by_id.get(&**pkg_id).cloned().unwrap_or_default(),
            parent_ids.pushed(pkg_id.to_string()),
        ))
    }

    pub(super) fn resolve_deferred_children(
        &mut self,
        deferred: DeferredChildren<'_>,
        walk: &NodeWalkContext<'_>,
    ) -> ChildOutputs {
        let DeferredChildren { pkg_id, children, parent_ids, provider_children, depth } = deferred;
        let canonical_scc = self.canonical_scc();
        let child_aliases = ChildAliases::Deferred(children);
        let mut child_outputs = ChildOutputs::default();
        for repeated in [true, false] {
            for edge in children {
                if walk.parent_refs.contains_key(&edge.alias) != repeated
                    || Self::cuts_cycle_edge(&canonical_scc, pkg_id, &edge.pkg_id)
                {
                    continue;
                }
                let child_node_id = self.child_node_id_for_edge(edge, Some(provider_children));
                let child_output =
                    self.resolve_deferred_edge(edge, child_node_id, parent_ids, depth, walk);
                child_outputs.push(&edge.alias, child_output, &child_aliases, false);
            }
        }
        child_outputs
    }

    pub(super) fn resolve_deferred_edge(
        &mut self,
        edge: &ChildEdge,
        child_node_id: NodeId,
        parent_ids: &AncestorIds,
        parent_depth: i32,
        walk: &NodeWalkContext<'_>,
    ) -> NodeOutput {
        if self.tree.dependencies_tree.contains_key(&child_node_id) {
            return self.resolve_node(&child_node_id, walk);
        }
        self.resolve_deferred_child(&DeferredChildContext {
            edge,
            node_id: child_node_id,
            parent_ids,
            walk,
            depth: parent_depth + 1,
        })
    }

    pub(super) fn resolve_realized_children(
        &mut self,
        pkg_id: &Arc<str>,
        children_map: &BTreeMap<String, NodeId>,
        walk: &NodeWalkContext<'_>,
    ) -> ChildOutputs {
        let canonical_scc = self.canonical_scc();
        let child_aliases = ChildAliases::Realized(children_map);
        let mut child_outputs = ChildOutputs::default();
        for repeated in [true, false] {
            for (alias, child_node_id) in children_map {
                if walk.parent_refs.contains_key(alias) != repeated {
                    continue;
                }
                // A canonical back-edge child is record-only: it is in the
                // realized map for the snapshot edge, but the position walk
                // contributes nothing through it, and the record pass remaps
                // it to the target's shared canonical occurrence. The drain
                // still walks it once at importer context — eagerly realized
                // trees reach their back-edge subtrees only through that
                // queue.
                if self.tree.dependencies_tree.get(child_node_id).is_some_and(|child| {
                    Self::cuts_cycle_edge(&canonical_scc, pkg_id, &child.resolved_package_id)
                }) {
                    continue;
                }
                let child_output = self.resolve_node(child_node_id, walk);
                child_outputs.push(alias, child_output, &child_aliases, !self.discovery);
            }
        }
        child_outputs
    }

    /// Record this walk's outcome in the per-`pkgIdWithPatchHash` caches.
    /// Pure subtrees go in [`Walker::pure_pkgs`] for the fast-path early
    /// return at the top of [`Walker::resolve_node`]; non-pure subtrees push
    /// a [`PeersCacheItem`] so a future visit with a compatible parent
    /// context can short-circuit via [`Walker::find_hit`]. The canonical
    /// cycle gate makes every occurrence of a package see the same subtree,
    /// so a verdict is a plain function of the parent context.
    pub(super) fn record_walk_result(
        &mut self,
        node_id: &NodeId,
        pkg_id: &Arc<str>,
        result: &WalkResult<'_>,
    ) {
        if !self.discovery {
            self.node_external_peers.insert(node_id.clone(), Arc::clone(result.all_resolved_peers));
            self.node_missing_peers.insert(node_id.clone(), Arc::clone(result.all_missing_peers));
            self.node_missing_peers_of_children
                .insert(node_id.clone(), Arc::clone(result.missing_peers_of_children));
        }
        if result.is_pure {
            self.pure_pkgs.insert(pkg_id.to_string(), result.dep_path.clone());
            return;
        }
        self.retained_peer_node_ids.extend(result.all_resolved_peers.values().cloned());
        self.peers_cache.entry(pkg_id.to_string()).or_default().push(PeersCacheItem {
            owner_node_id: node_id.clone(),
            dep_path: result.dep_path.clone(),
            resolved_peers: Arc::clone(result.all_resolved_peers),
            missing_peers: Arc::clone(result.all_missing_peers),
            missing_peers_of_children: Arc::clone(result.missing_peers_of_children),
            subtree_missing_by_pkg: result.subtree_missing_by_pkg.clone(),
        });
    }

    /// Build the [`ParentRefs`] map that descendants of this node see:
    /// the parent's view, plus the node's own peer-relevant children,
    /// plus the pins the wanted lockfile locked in. Kept behind `Arc`
    /// copy-on-write: most nodes contribute nothing, so they pass the
    /// parent's map down by refcount instead of cloning it — the
    /// per-node map clones dominated the walker's CPU time on
    /// peer-heavy workspaces.
    pub(super) fn build_child_parent_refs(
        &self,
        node_id: &NodeId,
        pkg: &ResolvedPackage,
        parent_parent_refs: &Arc<ParentRefs>,
        provider_children: &BTreeMap<String, NodeId>,
        parent_node_ids: &SharedChain<NodeId>,
    ) -> ChildParentRefs {
        let mut refs_changed = false;
        let mut child_parent_refs = Arc::clone(parent_parent_refs);

        let new_parent_refs = self.peer_provider_parent_refs(provider_children);
        if !new_parent_refs.is_empty() {
            refs_changed = true;
            let refs = Arc::make_mut(&mut child_parent_refs);
            self.apply_own_parent_refs(refs, &new_parent_refs, node_id);
        }

        let locked_pins =
            self.locked_peer_context_pins(node_id, pkg, &child_parent_refs, parent_node_ids);
        if !locked_pins.is_empty() {
            refs_changed = true;
            let refs = Arc::make_mut(&mut child_parent_refs);
            for (name, parent_ref) in locked_pins {
                refs.insert(name, parent_ref);
            }
        }

        ChildParentRefs { refs: child_parent_refs, own: new_parent_refs, changed: refs_changed }
    }

    /// What this node itself contributes to its descendants' parent context:
    /// its peer-relevant children.
    pub(super) fn peer_provider_parent_refs(
        &self,
        provider_children: &BTreeMap<String, NodeId>,
    ) -> ParentRefs {
        let mut new_parent_refs = ParentRefs::default();
        for (alias, child_node_id) in provider_children {
            let Some(child_tree) = self.tree.dependencies_tree.get(child_node_id) else { continue };
            let Some(child_pkg) = self.tree.packages.get(&child_tree.resolved_package_id) else {
                continue;
            };
            insert_parent_ref(
                &mut new_parent_refs,
                alias,
                child_node_id.clone(),
                child_pkg,
                child_tree.depth,
            );
        }
        new_parent_refs
    }

    /// Overlay this node's own providers onto the inherited parent context.
    /// A name the inherited context already binds is only shadowed when the
    /// two providers disagree, and the shadowing occurrence is bumped so the
    /// two stay distinguishable.
    pub(super) fn apply_own_parent_refs(
        &self,
        refs: &mut ParentRefs,
        new_parent_refs: &ParentRefs,
        node_id: &NodeId,
    ) {
        // Built only when a name collision actually consults it — the
        // common no-collision node never pays for the extra map clone.
        let mut refs_with_new: Option<ParentRefs> = None;
        for (name, mut new_parent_ref) in new_parent_refs.clone() {
            if let Some(existing) = refs.get(&name) {
                let with_new = refs_with_new.get_or_insert_with(|| {
                    let mut with_new = refs.clone();
                    with_new.extend(new_parent_refs.clone());
                    with_new
                });
                if !self.parent_refs_match(existing, &new_parent_ref)
                    || self.inherited_parent_pkg_breaks_peer_diamond(
                        with_new,
                        existing,
                        &new_parent_ref,
                        node_id,
                    )
                {
                    new_parent_ref.occurrence = existing.occurrence + 1;
                    refs.insert(name, new_parent_ref);
                }
            } else {
                refs.insert(name, new_parent_ref);
            }
        }
    }
}
