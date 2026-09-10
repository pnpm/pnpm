use super::{
    AncestorIds, Arc, BTreeMap, DependenciesTreeNode, HashMap, HashSet, NodeId, PeerDep,
    PkgNameVerPeer, TreeCtx, UpdateReuseScope, lock_recoverable,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) struct ChildrenOwner {
    pub(super) update_active: bool,
    pub(super) depth: i32,
    pub(in super::super) importer_order: usize,
    pub(super) parent_path: Vec<String>,
    pub(in super::super) importer_id: String,
}

impl ChildrenOwner {
    pub(super) fn wins_over(&self, other: &Self) -> bool {
        if self.update_active != other.update_active {
            return self.update_active;
        }
        (&self.depth, &self.importer_order, &self.parent_path)
            < (&other.depth, &other.importer_order, &other.parent_path)
    }
}

/// What the current children owner of a `pkgIdWithPatchHash` decided
/// for it. Both halves are per-occurrence inputs the walk has to settle
/// on one answer for, so they are claimed together, under one lock.
#[derive(Debug, Clone)]
pub(super) struct ChildrenOwnerEntry {
    pub(in super::super) owner: ChildrenOwner,
    /// The owner occurrence's peer-shadowed `dependencies` (see
    /// [`peer_shadowed_dependencies`]). Which names are shadowed
    /// depends on the parent scope, which differs per occurrence;
    /// pnpm lets the first occurrence to resolve decide, which is
    /// arrival-ordered. The deterministic children owner decides here
    /// instead, so the same graph always yields the same lockfile.
    ///
    /// [`peer_shadowed_dependencies`]: crate::parent_pkg_aliases::peer_shadowed_dependencies
    pub(in super::super) peer_shadowed: Arc<HashSet<String>>,
}

/// A package's recorded children together with what they were resolved
/// under. The two travel in one entry so a reader that accepts the
/// context cannot then expand edges another walk recorded.
#[derive(Debug, Clone)]
pub(in super::super) struct RecordedChildren {
    pub(in super::super) edges: Arc<Vec<crate::resolved_tree::ChildEdge>>,
    pub(super) context: RecordedChildrenContext,
}

/// What a recorded `children_by_id` entry was resolved under. A later
/// occurrence may expand from that entry instead of walking the
/// package's manifest again only when its own context equals this one:
/// each field can change which children the walk produces.
#[derive(Debug, Clone)]
pub(in super::super) struct RecordedChildrenContext {
    /// Dependencies the package's own `peerDependencies` shadow, which
    /// the walk drops from its children.
    pub(in super::super) peer_shadowed: Arc<HashSet<String>>,
    /// The prior-lockfile key whose snapshot pinned the children, if the
    /// walk reused one.
    pub(in super::super) prior_key: Option<PkgNameVerPeer>,
    /// Whether the resolving importer had an active update policy, which
    /// re-resolves what a keep-all importer reuses.
    pub(in super::super) update_active: bool,
}

impl RecordedChildrenContext {
    /// Whether a walk under `other` would re-resolve what the prior
    /// lockfile pinned for this recording — the churn reuse exists to
    /// avoid, and which leaves the occurrences that realized the
    /// pinned subtree pointing at children the record no longer holds.
    ///
    /// An update policy is the one thing that re-resolves a pin on
    /// purpose, so a walk under one is never held to the pins.
    pub(in super::super) fn pins_children_over(&self, other: &Self) -> bool {
        self.peer_shadowed == other.peer_shadowed
            && self.prior_key.is_some()
            && other.prior_key.is_none()
            && !other.update_active
    }

    /// Two contexts produce the same children.
    ///
    /// The preferred-versions overlay is deliberately not part of the
    /// test. `children_by_id` records one child list per package id,
    /// and every occurrence that does not own the children already
    /// expands from it whatever its own overlay says — the overlay
    /// only ever decides which occurrence's versions get *recorded*.
    /// Requiring identical overlays here would mean requiring identical
    /// parents, which the occurrences that race never have.
    pub(in super::super) fn produces_same_children_as(&self, other: &Self) -> bool {
        self.peer_shadowed == other.peer_shadowed
            && self.prior_key == other.prior_key
            && self.update_active == other.update_active
    }
}

pub(in super::super) struct ChildrenOwnerClaim {
    pub(in super::super) owner: ChildrenOwner,
    pub(in super::super) owns_children: bool,
    /// The shadowed-dependency set in force for the package id — this
    /// occurrence's when it won the claim, the standing owner's when it
    /// lost. See [`ChildrenOwnerEntry::peer_shadowed`].
    pub(in super::super) peer_shadowed: Arc<HashSet<String>>,
    /// A winning claim displaced an owner whose shadowed-dependency set
    /// equals this occurrence's, so the other occurrences' realized
    /// children — including a subtree reused from the prior lockfile —
    /// stay valid *as far as the claim can tell*, and the winner skips
    /// the lazy-flip (and the engine-invalidating rewrite signal) it
    /// would otherwise broadcast. The walk it then runs can still land
    /// on different edges, which
    /// [`ChildrenRecording::PublishedOverStale`] reports instead.
    ///
    /// This is a narrower question than whether the *recorded* children
    /// can be reused instead of walked; that one is
    /// [`fn@recorded_children_match`], which compares the full
    /// resolution context.
    pub(in super::super) children_context_unchanged: bool,
}

/// `peer_shadowed` is this occurrence's own set; it is installed as the
/// package id's set only when the occurrence wins the claim, so the
/// losing occurrences of a concurrent claim all read back the winner's.
pub(in super::super) fn claim_children_owner(
    ctx: &TreeCtx,
    pkg_id: &str,
    depth: i32,
    ancestor_ids: &[String],
    peer_shadowed: HashSet<String>,
) -> ChildrenOwnerClaim {
    let owner = ChildrenOwner {
        update_active: !matches!(ctx.update_reuse_scope(), UpdateReuseScope::All),
        depth,
        importer_order: ctx.importer_order,
        parent_path: ancestor_ids.to_vec(),
        importer_id: ctx.importer_id.clone(),
    };
    let (owns_children, peer_shadowed, children_context_unchanged) = {
        let mut owners = lock_recoverable(&ctx.workspace.children_owner_by_id);
        match owners.get(pkg_id) {
            Some(existing) if !owner.wins_over(&existing.owner) => {
                (false, Arc::clone(&existing.peer_shadowed), false)
            }
            existing => {
                let children_context_unchanged =
                    existing.is_some_and(|entry| *entry.peer_shadowed == peer_shadowed);
                let peer_shadowed = Arc::new(peer_shadowed);
                owners.insert(
                    Arc::from(pkg_id),
                    ChildrenOwnerEntry {
                        owner: owner.clone(),
                        peer_shadowed: Arc::clone(&peer_shadowed),
                    },
                );
                (true, peer_shadowed, children_context_unchanged)
            }
        }
    };
    if owns_children {
        let mut first_importer = lock_recoverable(&ctx.workspace.first_importer_by_pkg);
        if first_importer.map().get(pkg_id) != Some(&owner.importer_id) {
            first_importer.map_mut().insert(pkg_id.to_string(), owner.importer_id.clone());
        }
    }
    ChildrenOwnerClaim { owner, owns_children, peer_shadowed, children_context_unchanged }
}

/// Whether this occurrence is the first to offer to warm its package's
/// children, so the speculative resolutions run once per package
/// rather than once per occurrence of it.
pub(in super::super) fn claim_children_warmup(ctx: &TreeCtx, pkg_id: &str) -> bool {
    let mut warmed = lock_recoverable(&ctx.workspace.warmed_children_by_id);
    // Every occurrence of a package offers, and all but the first are
    // turned away, so the owned key is built only for the one that
    // takes the warmup.
    !warmed.contains(pkg_id) && warmed.insert(Arc::from(pkg_id.to_string()))
}

/// Whether this package's recorded children were resolved under
/// `context`, and can therefore be expanded from instead of walked
/// again. The walk that recorded them need not be the one owning the
/// children now, which is why the comparison is against the recorded
/// context rather than against the standing claim.
pub(in super::super) fn recorded_children_match(
    ctx: &TreeCtx,
    pkg_id: &str,
    context: &RecordedChildrenContext,
) -> bool {
    lock_recoverable(&ctx.workspace.children_by_id).get(pkg_id).is_some_and(|recorded| {
        recorded.context.produces_same_children_as(context)
            || recorded.context.pins_children_over(context)
    })
}

/// What [`fn@record_children`] did with a walk's child edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum ChildrenRecording {
    /// This walk's ownership lapsed before it could publish, so the
    /// standing owner's children stand. Its node stays lazy and expands
    /// from whatever that owner recorded.
    Declined,
    /// Published, and the realized children every other occurrence node
    /// holds still stand.
    Published,
    /// Published over edges the other occurrence nodes realized, whose
    /// children are now stale.
    PublishedOverStale,
}

impl ChildrenRecording {
    /// The children to hang on the recording walk's own node, plus
    /// whether the recording staled the children the package's other
    /// occurrence nodes realized — the flag that gates
    /// [`fn@make_non_owner_nodes_lazy`].
    pub(in super::super) fn into_children(
        self,
        realized: BTreeMap<String, NodeId>,
        parent_ids: &Arc<Vec<String>>,
    ) -> (crate::resolved_tree::TreeChildren, bool) {
        match self {
            ChildrenRecording::Declined => (lazy_children(parent_ids), false),
            ChildrenRecording::Published => {
                (crate::resolved_tree::TreeChildren::Realized(std::sync::Arc::new(realized)), false)
            }
            ChildrenRecording::PublishedOverStale => {
                (crate::resolved_tree::TreeChildren::Realized(std::sync::Arc::new(realized)), true)
            }
        }
    }
}

/// Children a node expands from the standing owner's recording, under
/// its own `parent_ids` cycle break.
pub(in super::super) fn lazy_children(
    parent_ids: &Arc<Vec<String>>,
) -> crate::resolved_tree::TreeChildren {
    crate::resolved_tree::TreeChildren::Lazy {
        parent_ids: AncestorIds::from(Arc::clone(parent_ids)),
    }
}

/// Publish a package's children together with the context that
/// produced them, and report what that did.
///
/// The ownership check and the comparison against the standing
/// recording happen under the same lock as the write: a claim that
/// landed while this walk ran has its own children to publish, and an
/// older walk finishing afterwards would otherwise overwrite them.
pub(in super::super) fn record_children(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner: &ChildrenOwner,
    edges: Vec<crate::resolved_tree::ChildEdge>,
    context: RecordedChildrenContext,
) -> ChildrenRecording {
    let recording = {
        let owners = lock_recoverable(&ctx.workspace.children_owner_by_id);
        if owners.get(pkg_id).is_none_or(|entry| entry.owner != *owner) {
            return ChildrenRecording::Declined;
        }
        let mut children = lock_recoverable(&ctx.workspace.children_by_id);
        let recording = match children.get(pkg_id) {
            // Nothing recorded yet, so no occurrence node can hold
            // realized children of this package to stale.
            None => ChildrenRecording::Published,
            // A recording the prior lockfile pinned outlives a fresh
            // walk's answer, so this walk publishes nothing and reads
            // the pinned children like every occurrence that reused the
            // subtree. Publishing over them would re-resolve the open
            // ranges reuse exists to hold still, and would leave those
            // occurrences realizing children the record no longer
            // holds. This comes before the equal-edge arm because
            // republishing even the same edges would carry this walk's
            // unpinned context onto the record, leaving the next fresh
            // walk to land on different edges nothing to hold it back.
            Some(recorded) if recorded.context.pins_children_over(&context) => {
                return ChildrenRecording::Declined;
            }
            Some(recorded) if *recorded.edges == edges => ChildrenRecording::Published,
            Some(_) => ChildrenRecording::PublishedOverStale,
        };
        let edges = Arc::new(edges);
        if ctx.workspace.finalized_package.is_some() {
            update_parent_index(
                &mut lock_recoverable(&ctx.workspace.parents_by_id),
                pkg_id,
                children.get(pkg_id).map(|recorded| recorded.edges.as_slice()),
                &edges,
            );
        }
        children.insert(Arc::from(pkg_id.to_string()), RecordedChildren { edges, context });
        recording
    };
    ctx.workspace.record_children_by_id_write(pkg_id);
    ctx.workspace.note_finalization_candidate(pkg_id);
    recording
}

/// Move `pkg_id` in the reverse parent index from the children it
/// recorded before (`previous`) to the ones it records now (`next`).
/// Recording the same edges again is a no-op, and a child dropped
/// from the record no longer lists `pkg_id` as a parent.
pub(super) fn update_parent_index(
    parents_by_id: &mut HashMap<Arc<str>, HashSet<Arc<str>>>,
    pkg_id: &str,
    previous: Option<&[crate::resolved_tree::ChildEdge]>,
    next: &[crate::resolved_tree::ChildEdge],
) {
    if previous.is_some_and(|previous| previous == next) {
        return;
    }
    let kept: HashSet<&str> = next.iter().map(|edge| edge.pkg_id.as_ref()).collect();
    for edge in previous.into_iter().flatten() {
        if kept.contains(edge.pkg_id.as_ref()) {
            continue;
        }
        if let Some(parents) = parents_by_id.get_mut(&edge.pkg_id) {
            parents.remove(pkg_id);
            if parents.is_empty() {
                parents_by_id.remove(&edge.pkg_id);
            }
        }
    }
    for edge in next {
        parents_by_id.entry(Arc::clone(&edge.pkg_id)).or_default().insert(Arc::from(pkg_id));
    }
}

/// Seed the peer-walker's `parentPkgs` filter with the names a
/// resolved package declares as peers.
pub(in super::super) fn register_peer_dep_names(
    ctx: &TreeCtx,
    peer_dependencies: &BTreeMap<String, PeerDep>,
) {
    let mut all_peers = lock_recoverable(&ctx.workspace.all_peer_dep_names);
    for name in peer_dependencies.keys() {
        if all_peers.insert(name.clone()) {
            ctx.workspace.record_peer_dep_name(name);
        }
    }
}

pub(in super::super) fn is_current_children_owner(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner: &ChildrenOwner,
) -> bool {
    lock_recoverable(&ctx.workspace.children_owner_by_id)
        .get(pkg_id)
        .is_some_and(|current| current.owner == *owner)
}

pub(in super::super) fn remember_node_parent_ids(
    ctx: &TreeCtx,
    node_id: &NodeId,
    parent_ids: Arc<Vec<String>>,
) {
    lock_recoverable(&ctx.workspace.node_parent_ids_by_id).insert(node_id.clone(), parent_ids);
}

/// Record an occurrence node in the shared tree (lowering the depth of
/// a revisited leaf) and, on first insertion, in the per-package
/// reverse index [`fn@make_non_owner_nodes_lazy`] flips through.
pub(in super::super) fn insert_tree_node(
    ctx: &TreeCtx,
    node_id: NodeId,
    pkg_id: &str,
    children: crate::resolved_tree::TreeChildren,
    depth: i32,
) {
    let mut written = true;
    let inserted = match lock_recoverable(&ctx.workspace.dependencies_tree).entry(node_id.clone()) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            written = entry.get().depth > depth;
            if written {
                entry.get_mut().depth = depth;
            }
            false
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(DependenciesTreeNode::new(
                Arc::from(pkg_id.to_string()),
                children,
                depth,
                true,
            ));
            true
        }
    };
    if written {
        ctx.workspace.record_tree_node_write(&node_id);
    }
    if inserted {
        lock_recoverable(&ctx.workspace.nodes_by_pkg_id)
            .entry(Arc::from(pkg_id.to_string()))
            .or_default()
            .push(node_id);
    }
}

pub(in super::super) fn make_non_owner_nodes_lazy(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner_node_id: &NodeId,
) {
    let pkg_nodes = match lock_recoverable(&ctx.workspace.nodes_by_pkg_id).get(pkg_id) {
        Some(nodes) => nodes.clone(),
        None => return,
    };
    // Collect the parent chains first so the two locks are never held
    // together.
    let parent_ids_by_node: Vec<(NodeId, Arc<Vec<String>>)> = {
        let parent_ids = lock_recoverable(&ctx.workspace.node_parent_ids_by_id);
        pkg_nodes
            .into_iter()
            .filter(|node_id| node_id != owner_node_id)
            .filter_map(|node_id| {
                let ids = Arc::clone(parent_ids.get(&node_id)?);
                Some((node_id, ids))
            })
            .collect()
    };
    let mut tree = lock_recoverable(&ctx.workspace.dependencies_tree);
    let mut rewritten = Vec::new();
    for (node_id, parent_ids) in parent_ids_by_node {
        // An occurrence already reading the owner's children needs no
        // rewrite — and must not report one, since the signal makes the
        // discovery engine rebuild from scratch. In a peer-heavy graph
        // most occurrences of a package are already lazy.
        if let Some(node) = tree.get_mut(&node_id)
            && !matches!(node.children, crate::resolved_tree::TreeChildren::Lazy { .. })
        {
            node.children = crate::resolved_tree::TreeChildren::Lazy {
                parent_ids: AncestorIds::from(parent_ids),
            };
            rewritten.push(node_id);
        }
    }
    drop(tree);
    let rewrote_any = !rewritten.is_empty();
    for node_id in &rewritten {
        ctx.workspace.record_tree_node_write(node_id);
    }
    if rewrote_any {
        ctx.workspace.record_children_rewrite();
    }
}
