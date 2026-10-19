use super::{
    super::{
        reuse::reuse_key_matches_peer_scope,
        walk::{closes_cycle, node_id_for},
    },
    AncestorPkgIds, Arc, BTreeMap, ChildrenOwner, HashMap, NodeId, ParentPkgAliases,
    PkgNameVerPeer, RecordedChildren, RecordedChildrenContext, TreeChildren, TreeCtx,
    children_ownership::{insert_tree_node, make_non_owner_nodes_lazy, record_first_importer},
    lock_recoverable,
};
use crate::peer_provider::peer_shadowed_dependencies;

/// Whether this package's recorded children were resolved under
/// `context`, and can therefore be expanded from instead of walked
/// again. The walk that recorded them need not be the one owning the
/// children now, which is why the comparison is against the recorded
/// context rather than against the standing claim.
pub(in super::super) fn recorded_children_match(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner: &ChildrenOwner,
    context: &RecordedChildrenContext,
) -> bool {
    let base_matches = lock_recoverable(&ctx.workspace.children.by_id)
        .get(pkg_id)
        .is_some_and(|recorded| {
            recorded.context.peer_shadowed == context.peer_shadowed
                && ((recorded.context.prior_key == context.prior_key
                    && recorded.context.update_active == context.update_active)
                    || recorded.context.pins_children_over(context))
        });
    base_matches && !subtree_changes_owner_split(ctx, pkg_id, owner, &context.children_scope)
}

/// The prior-lockfile key this package's recorded children were pinned
/// under, when a later fresh owner re-walks them so its own children
/// keep their pins where their peer-provider split still matches.
pub(in super::super) fn recorded_prior_key(ctx: &TreeCtx, pkg_id: &str) -> Option<PkgNameVerPeer> {
    lock_recoverable(&ctx.workspace.children.by_id)
        .get(pkg_id)
        .and_then(|recorded| recorded.context.prior_key.clone())
}

struct OwnedParent {
    pkg_id: Arc<str>,
    node_id: NodeId,
    depth: i32,
    parent_path: AncestorPkgIds,
}

struct ChildCandidate {
    pkg_id: Arc<str>,
    node_id: NodeId,
    depth: i32,
    parent_path: AncestorPkgIds,
    is_link: bool,
}

fn recorded_parent_edges(
    ctx: &TreeCtx,
    pkg_id: &str,
) -> Option<Arc<Vec<crate::resolved_tree::ChildEdge>>> {
    lock_recoverable(&ctx.workspace.children.by_id)
        .get(pkg_id)
        .filter(|recorded| !recorded.edges.is_empty())
        .map(|recorded| Arc::clone(&recorded.edges))
}

fn realize_parent(ctx: &TreeCtx, parent: OwnedParent, candidates: &mut Vec<ChildCandidate>) {
    let mut realized = BTreeMap::new();
    if let Some(edges) = recorded_parent_edges(ctx, &parent.pkg_id) {
        push_parent_children(ctx, &parent, &edges, &mut realized, candidates);
    }
    make_non_owner_nodes_lazy(ctx, &parent.pkg_id, &parent.node_id);
    insert_tree_node(
        ctx,
        parent.node_id,
        &parent.pkg_id,
        TreeChildren::Realized(Arc::new(realized)),
        parent.depth,
    );
}

fn push_parent_children(
    ctx: &TreeCtx,
    parent: &OwnedParent,
    edges: &[crate::resolved_tree::ChildEdge],
    realized: &mut BTreeMap<String, NodeId>,
    candidates: &mut Vec<ChildCandidate>,
) {
    let next_path: AncestorPkgIds = Arc::new(
        parent.parent_path
            .iter()
            .cloned()
            .chain(std::iter::once(Arc::clone(&parent.pkg_id)))
            .collect(),
    );
    let packages = lock_recoverable(&ctx.workspace.tree.packages);
    for edge in edges {
        if closes_cycle(&next_path, &edge.pkg_id) {
            continue;
        }
        let is_link = edge.pkg_id.starts_with("link:");
        let is_leaf = is_link || packages.get(&edge.pkg_id).is_some_and(|pkg| pkg.is_leaf);
        let child_node_id = node_id_for(is_leaf, &edge.pkg_id);
        realized.insert(edge.alias.clone(), child_node_id.clone());
        candidates.push(ChildCandidate {
            pkg_id: Arc::clone(&edge.pkg_id),
            node_id: child_node_id,
            depth: parent.depth + 1,
            parent_path: Arc::clone(&next_path),
            is_link,
        });
    }
}

/// Advance children ownership and realize
/// [`DependenciesTreeNode`](crate::resolved_tree::DependenciesTreeNode)s
/// through `pkg_id`'s recorded subtree when a winning owner reuses
/// `pkg_id`'s recorded children instead of walking the subtree again.
pub(in super::super) fn claim_recorded_subtree_owners(
    ctx: &TreeCtx,
    pkg_id: &Arc<str>,
    node_id: &NodeId,
    owner: &ChildrenOwner,
) {
    let mut current_parents = vec![OwnedParent {
        pkg_id: Arc::clone(pkg_id),
        node_id: node_id.clone(),
        depth: owner.depth,
        parent_path: Arc::clone(&owner.parent_path),
    }];
    while !current_parents.is_empty() {
        let mut candidates = Vec::new();
        for parent in current_parents {
            realize_parent(ctx, parent, &mut candidates);
        }
        let best_by_id = candidate_level_winners(&candidates);
        current_parents = candidates
            .into_iter()
            .enumerate()
            .filter_map(|(idx, cand)| settle_child_candidate(ctx, owner, idx, cand, &best_by_id))
            .collect();
    }
}

fn candidate_level_winners(candidates: &[ChildCandidate]) -> HashMap<Arc<str>, usize> {
    let mut best_by_id: HashMap<Arc<str>, usize> = HashMap::default();
    for (idx, cand) in candidates.iter().enumerate() {
        if cand.is_link {
            continue;
        }
        let best = *best_by_id
            .entry(Arc::clone(&cand.pkg_id))
            .or_insert(idx);
        if (&candidates[best].depth, &candidates[best].parent_path)
            > (&cand.depth, &cand.parent_path)
        {
            best_by_id.insert(Arc::clone(&cand.pkg_id), idx);
        }
    }
    best_by_id
}

fn settle_child_candidate(
    ctx: &TreeCtx,
    owner: &ChildrenOwner,
    idx: usize,
    cand: ChildCandidate,
    best_by_id: &HashMap<Arc<str>, usize>,
) -> Option<OwnedParent> {
    if cand.is_link {
        insert_tree_node(
            ctx,
            cand.node_id,
            &cand.pkg_id,
            TreeChildren::Realized(Arc::new(BTreeMap::new())),
            -1,
        );
        return None;
    }
    let won =
        best_by_id.get(&cand.pkg_id) == Some(&idx) && try_claim_candidate_owner(ctx, owner, &cand);
    if won {
        Some(OwnedParent {
            pkg_id: cand.pkg_id,
            node_id: cand.node_id,
            depth: cand.depth,
            parent_path: cand.parent_path,
        })
    } else {
        insert_tree_node(ctx, cand.node_id, &cand.pkg_id, TreeChildren::Lazy, cand.depth);
        None
    }
}

fn try_claim_candidate_owner(ctx: &TreeCtx, owner: &ChildrenOwner, cand: &ChildCandidate) -> bool {
    let cand_owner = ChildrenOwner {
        update_active: owner.update_active,
        depth: cand.depth,
        importer_order: owner.importer_order,
        parent_path: Arc::clone(&cand.parent_path),
        importer_id: owner.importer_id.clone(),
    };
    let won = {
        let mut owners = lock_recoverable(&ctx.workspace.children.owner_by_id);
        match owners.get_mut(&cand.pkg_id) {
            Some(existing) if cand_owner.wins_over(&existing.owner) => {
                existing.owner = cand_owner.clone();
                true
            }
            _ => false,
        }
    };
    if won {
        record_first_importer(ctx, &cand.pkg_id, &cand_owner);
    }
    won
}

struct LevelCandidate {
    pkg_id: Arc<str>,
    depth: i32,
    parent_path: AncestorPkgIds,
    scope: Arc<ParentPkgAliases>,
}

/// Whether any descendant in `pkg_id`'s recorded subtree that this walk's
/// `owner` would own (or tie for ownership of) has a peer-shadowed
/// dependency split under `children_scope` that differs from its standing
/// recording.
///
/// The subtree is already in `children_by_id` and `packages`, so the check
/// walks those in-memory maps level by level without touching the resolver.
pub(super) fn subtree_changes_owner_split(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner: &ChildrenOwner,
    children_scope: &Arc<ParentPkgAliases>,
) -> bool {
    if lock_recoverable(&ctx.workspace.children.shadowable_dep_names).is_empty() {
        return false;
    }
    let Some(root_recorded) = recorded_children_entry(ctx, pkg_id) else {
        return false;
    };
    let root_id: Arc<str> = Arc::from(pkg_id);
    let mut current_level = Vec::new();
    push_recorded_children(
        &root_id,
        owner.depth,
        &owner.parent_path,
        children_scope,
        &root_recorded,
        &mut current_level,
    );
    while !current_level.is_empty() {
        let mut next_level = Vec::new();
        if advance_split_level(ctx, owner, &current_level, &mut next_level) {
            return true;
        }
        current_level = next_level;
    }
    false
}

fn recorded_children_entry(ctx: &TreeCtx, pkg_id: &str) -> Option<RecordedChildren> {
    lock_recoverable(&ctx.workspace.children.by_id).get(pkg_id).cloned()
}

fn push_recorded_children(
    parent_id: &Arc<str>,
    parent_depth: i32,
    parent_path: &AncestorPkgIds,
    parent_scope: &Arc<ParentPkgAliases>,
    recorded: &RecordedChildren,
    out: &mut Vec<LevelCandidate>,
) {
    if recorded.edges.is_empty() {
        return;
    }
    let next_path: AncestorPkgIds = Arc::new(
        parent_path
            .iter()
            .cloned()
            .chain(std::iter::once(Arc::clone(parent_id)))
            .collect(),
    );
    for edge in recorded.edges.iter() {
        if next_path
            .iter()
            .any(|ancestor| ancestor == &edge.pkg_id)
        {
            continue;
        }
        out.push(LevelCandidate {
            pkg_id: Arc::clone(&edge.pkg_id),
            depth: parent_depth + 1,
            parent_path: Arc::clone(&next_path),
            scope: Arc::clone(parent_scope),
        });
    }
}

fn level_winner_indices(current_level: &[LevelCandidate]) -> Vec<usize> {
    let mut best_by_id: HashMap<Arc<str>, usize> = HashMap::default();
    for (idx, cand) in current_level.iter().enumerate() {
        let best = *best_by_id
            .entry(Arc::clone(&cand.pkg_id))
            .or_insert(idx);
        if (&current_level[best].depth, &current_level[best].parent_path)
            > (&cand.depth, &cand.parent_path)
        {
            best_by_id.insert(Arc::clone(&cand.pkg_id), idx);
        }
    }
    let mut winners: Vec<usize> = best_by_id.into_values().collect();
    winners.sort_unstable();
    winners
}

fn advance_split_level(
    ctx: &TreeCtx,
    owner: &ChildrenOwner,
    current_level: &[LevelCandidate],
    next_level: &mut Vec<LevelCandidate>,
) -> bool {
    for idx in level_winner_indices(current_level) {
        if check_split_winner(ctx, owner, &current_level[idx], next_level) {
            return true;
        }
    }
    false
}

fn check_split_winner(
    ctx: &TreeCtx,
    owner: &ChildrenOwner,
    cand: &LevelCandidate,
    next_level: &mut Vec<LevelCandidate>,
) -> bool {
    let cand_owner = ChildrenOwner {
        update_active: owner.update_active,
        depth: cand.depth,
        importer_order: owner.importer_order,
        parent_path: Arc::clone(&cand.parent_path),
        importer_id: owner.importer_id.clone(),
    };
    let wins_or_ties = lock_recoverable(&ctx.workspace.children.owner_by_id)
        .get(&cand.pkg_id)
        .is_none_or(|existing| {
            cand_owner.wins_over(&existing.owner) || cand_owner == existing.owner
        });
    if !wins_or_ties {
        return false;
    }
    let recorded = recorded_children_entry(ctx, &cand.pkg_id);
    if candidate_changes_split(ctx, &cand.pkg_id, &cand.scope, recorded.as_ref()) {
        return true;
    }
    if let Some(child_recorded) = recorded {
        let child_scope = cand.scope.extend(
            child_recorded.edges
                .iter()
                .map(|edge| edge.alias.clone())
                .collect(),
        );
        push_recorded_children(
            &cand.pkg_id,
            cand.depth,
            &cand.parent_path,
            &child_scope,
            &child_recorded,
            next_level,
        );
    }
    false
}

fn candidate_changes_split(
    ctx: &TreeCtx,
    pkg_id: &Arc<str>,
    scope: &ParentPkgAliases,
    recorded: Option<&RecordedChildren>,
) -> bool {
    let manifest = {
        let packages = lock_recoverable(&ctx.workspace.tree.packages);
        packages
            .get(pkg_id)
            .and_then(|pkg| pkg.result().package.manifest.clone())
    };
    if let Some(recorded) = recorded
        && recorded.context.reused
        && let Some(prior_key) = recorded.context.prior_key.as_ref()
    {
        return !reuse_key_matches_peer_scope(ctx, manifest.as_deref(), prior_key, scope);
    }
    let standing = lock_recoverable(&ctx.workspace.children.owner_by_id)
        .get(pkg_id)
        .map(|entry| Arc::clone(&entry.peer_shadowed))
        .or_else(|| recorded.map(|rec| Arc::clone(&rec.context.peer_shadowed)));
    let Some(standing) = standing else {
        return false;
    };
    let expected = peer_shadowed_dependencies(
        manifest.as_deref(),
        scope,
        ctx.workspace.policy.auto_install_peers,
    );
    expected != *standing
}
