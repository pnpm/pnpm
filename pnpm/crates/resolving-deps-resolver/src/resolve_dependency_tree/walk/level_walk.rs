use super::{
    Arc, BTreeMap, ChildSpec, ChildrenOwnerClaim, DirectDep, FrontierNode, HashMap, HashSet,
    NodeId, NodeSeed, ParentPkgAliases, PendingNode, PreferredVersionsOverlay,
    RecordedChildrenContext, ResolveDependencyTreeError, SeededNode,
    SkippedOptionalDependencyParent, TreeCtx, catalogs_for_children, claim_children_owner,
    extract_peer_dependencies, insert_tree_node, is_current_children_owner, lazy_children,
    lock_recoverable, make_non_owner_nodes_lazy, record_children, recorded_children_match,
    register_peer_dep_names, remember_node_parent_ids,
};

/// Settle children ownership across every occurrence one level seeded.
///
/// The ownership rank is `(update policy, depth, importer order,
/// parent path)` and a level shares the first three, so the
/// best-placed occurrence of a package is the one whose parent path
/// sorts first. Only that one claims: the losers cannot win against a
/// standing owner either, since they do not even outrank their own
/// level's winner.
pub(super) fn assign_level_owners<'seed>(
    ctx: &TreeCtx,
    seeds: impl Iterator<Item = &'seed mut NodeSeed>,
) -> Result<(), ResolveDependencyTreeError> {
    // Linked nodes resolve their dependencies as their own importer,
    // so they never own children here.
    let mut level: Vec<&mut Box<PendingNode>> = seeds
        .filter_map(|seed| match seed {
            NodeSeed::Pending(pending) if !pending.is_link => Some(pending),
            _ => None,
        })
        .collect();
    let winners: Vec<usize> = {
        let mut best: HashMap<&str, usize> = HashMap::default();
        for (index, pending) in level.iter().enumerate() {
            let best_so_far = *best.entry(pending.id.as_str()).or_insert(index);
            let standing = &level[best_so_far];
            // Depth joins the comparison even though one level shares
            // it, so this cannot drift from [`ChildrenOwner::wins_over`]
            // if a frontier ever carries more than one depth.
            if (standing.depth, &standing.parent_ancestors)
                > (pending.depth, &pending.parent_ancestors)
            {
                best.insert(pending.id.as_str(), index);
            }
        }
        let mut winners: Vec<usize> = best.into_values().collect();
        winners.sort_unstable();
        winners
    };
    for index in winners {
        let pending = &mut *level[index];
        let peer_shadowed = std::mem::take(&mut pending.peer_shadowed);
        let claim = claim_children_owner(
            ctx,
            &pending.id,
            pending.depth,
            &pending.parent_ancestors,
            peer_shadowed,
        );
        install_owner_peer_dependencies(ctx, pending, &claim)?;
        pending.claim = Some(claim);
    }
    Ok(())
}

/// Split the package's envelope into children and peers the way its
/// children owner reads its manifest. Which of a package's
/// dependencies its own `peerDependencies` shadow follows from the
/// scope the occurrence resolved in, so a package first seeded by one
/// occurrence and owned by another has to be re-split.
pub(super) fn install_owner_peer_dependencies(
    ctx: &TreeCtx,
    pending: &PendingNode,
    claim: &ChildrenOwnerClaim,
) -> Result<(), ResolveDependencyTreeError> {
    if pending.is_link || !claim.owns_children {
        return Ok(());
    }
    let peer_dependencies = extract_peer_dependencies(
        &pending.result,
        &claim.peer_shadowed,
        catalogs_for_children(ctx, pending.resolves_children_through_catalogs),
    )?;
    let mut packages = lock_recoverable(&ctx.workspace.packages);
    let Some(existing) = packages.get_mut(pending.id.as_str()) else { return Ok(()) };
    if existing.peer_dependencies == peer_dependencies {
        return Ok(());
    }
    existing.peer_dependencies = peer_dependencies.clone();
    drop(packages);
    register_peer_dep_names(ctx, &peer_dependencies);
    ctx.workspace.record_package_write(&pending.id);
    Ok(())
}

/// Give every occurrence of a settled level its tree node, and collect
/// the ones that still have to walk children of their own.
///
/// An occurrence walks only when it owns its package's children and
/// nothing has recorded them under its context; every other one reads
/// them from the owner's recording, under its own `parent_ids` cycle
/// break.
pub(super) fn settle_seeds(
    ctx: &TreeCtx,
    seeds: Vec<NodeSeed>,
    children_overlay: Option<&Arc<PreferredVersionsOverlay>>,
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Vec<FrontierNode> {
    let mut frontier = Vec::new();
    for seed in seeds {
        let NodeSeed::Pending(mut pending) = seed else { continue };
        let claim = pending.claim.take();
        // Linked nodes don't walk their manifest's deps — see the
        // `is_link` comment block in [`fn@resolve_node_seed`]. They get
        // an empty `Realized` map: a linked node has no children of its
        // own here.
        if pending.is_link {
            insert_walked_node(
                ctx,
                &pending,
                crate::resolved_tree::TreeChildren::Realized(std::sync::Arc::new(BTreeMap::new())),
            );
            continue;
        }
        let Some(claim) = claim.filter(|claim| claim.owns_children) else {
            let children = lazy_children(&pending.parent_ancestors);
            insert_walked_node(ctx, &pending, children);
            continue;
        };
        if !pending.resolves_children_through_catalogs
            && recorded_children_match(ctx, &pending.id, &children_context(ctx, &pending, &claim))
        {
            let children = lazy_children(&pending.parent_ancestors);
            insert_walked_node(ctx, &pending, children);
            continue;
        }
        frontier.push(FrontierNode {
            pending,
            claim,
            children_overlay: children_overlay.cloned(),
            children_pkg_aliases: Arc::clone(children_pkg_aliases),
        });
    }
    frontier
}

/// Record what each occurrence of a seeded level resolved for its
/// children, and settle that level's own seeds into the next frontier.
pub(super) fn settle_level(
    ctx: &TreeCtx,
    mut seeded: Vec<SeededNode>,
) -> Result<Vec<FrontierNode>, ResolveDependencyTreeError> {
    assign_level_owners(ctx, seeded.iter_mut().flat_map(|node| node.seeds.iter_mut()))?;
    let mut frontier = Vec::new();
    for node in seeded {
        let SeededNode {
            node: FrontierNode { pending, claim, .. },
            child_specs,
            seeds,
            grandchild_overlay,
            grandchild_pkg_aliases,
        } = node;
        let (children, others_stale) =
            record_walked_children(ctx, &pending, &claim, &child_specs, &seeds);
        insert_walked_node(ctx, &pending, children);
        if (others_stale || !claim.children_context_unchanged)
            && is_current_children_owner(ctx, &pending.id, &claim.owner)
        {
            make_non_owner_nodes_lazy(ctx, &pending.id, &pending.node_id);
        }
        frontier.extend(settle_seeds(
            ctx,
            seeds,
            grandchild_overlay.as_ref(),
            &grandchild_pkg_aliases,
        ));
    }
    super::super::finalized::announce_finalized_packages(ctx);
    Ok(frontier)
}

/// Publish the child edges one occurrence resolved, and report the
/// children to hang on its own tree node.
///
/// `children_by_id` records the resolved child pkg ids (not `NodeIds`)
/// plus the `optional` flag so lazy realisation can thread
/// `current_is_optional` correctly; the occurrence's own node keeps the
/// realized `(alias → NodeId)` map.
pub(super) fn record_walked_children(
    ctx: &TreeCtx,
    pending: &PendingNode,
    claim: &ChildrenOwnerClaim,
    child_specs: &[ChildSpec],
    seeds: &[NodeSeed],
) -> (crate::resolved_tree::TreeChildren, bool) {
    if !is_current_children_owner(ctx, &pending.id, &claim.owner) {
        return (lazy_children(&pending.parent_ancestors), false);
    }
    let optional_by_alias: HashMap<&str, bool> =
        child_specs.iter().map(|(name, _, optional, _)| (name.as_str(), *optional)).collect();
    let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
    for dep in seeds.iter().filter_map(seeded_dep) {
        let optional = optional_by_alias.get(dep.alias.as_str()).copied().unwrap_or(false);
        by_id.push(crate::resolved_tree::ChildEdge {
            alias: dep.alias.clone(),
            pkg_id: Arc::from(dep.id),
            optional,
        });
        realized.insert(dep.alias, dep.node_id);
    }
    record_children(ctx, &pending.id, &claim.owner, by_id, children_context(ctx, pending, claim))
        .into_children(realized, &pending.parent_ancestors)
}

/// The edge one seed contributes to its parent's children. `None` for
/// a seed the walk dropped — a cycle re-entry or a skipped optional.
pub(super) fn seeded_dep(seed: &NodeSeed) -> Option<DirectDep> {
    match seed {
        NodeSeed::Done(dep) => dep.clone(),
        NodeSeed::Pending(pending) => Some(DirectDep {
            alias: pending.alias.clone(),
            node_id: pending.node_id.clone(),
            id: pending.id.clone(),
        }),
    }
}

/// What a walk resolved its children under, which a later occurrence
/// compares its own against before reading them.
pub(super) fn children_context(
    ctx: &TreeCtx,
    pending: &PendingNode,
    claim: &ChildrenOwnerClaim,
) -> RecordedChildrenContext {
    RecordedChildrenContext {
        peer_shadowed: Arc::clone(&claim.peer_shadowed),
        prior_key: pending.prior_key.clone(),
        update_active: !matches!(ctx.update_reuse_scope(), super::super::UpdateReuseScope::All),
    }
}

/// Record an occurrence in the shared tree.
///
/// Repeat-visit leaves collapse onto one tree node; keep the
/// shallowest depth seen so downstream consumers that read
/// `tree_node.depth` (the peer pass folds it onto the graph node's
/// `depth`) take the minimum across visits. Per-occurrence counter ids
/// are unique by construction, so that only ever fires for leaves.
/// Linked nodes carry `depth = -1` so the peer-resolution pass
/// short-circuits them.
pub(super) fn insert_walked_node(
    ctx: &TreeCtx,
    pending: &PendingNode,
    children: crate::resolved_tree::TreeChildren,
) {
    let depth = if pending.is_link { -1 } else { pending.depth };
    remember_node_parent_ids(ctx, &pending.node_id, Arc::clone(&pending.parent_ancestors));
    insert_tree_node(ctx, pending.node_id.clone(), &pending.id, children, depth);
}

/// The install aliases one resolved level contributes to its
/// children's [`ParentPkgAliases`] scope. An edge the walk dropped (a
/// cycle re-entry, a skipped optional) contributes nothing, matching
/// pnpm's fold over the level's resolved addresses.
pub(in super::super) fn level_aliases(seeds: &[NodeSeed]) -> HashSet<String> {
    seeds
        .iter()
        .filter_map(|seed| match seed {
            NodeSeed::Pending(pending) => Some(pending.alias.clone()),
            NodeSeed::Done(Some(dep)) => Some(dep.alias.clone()),
            NodeSeed::Done(None) => None,
        })
        .collect()
}

/// The `(name → versions)` additions one resolved level contributes
/// to its children's preferred-versions overlay. Linked nodes carry no
/// `name_ver` and contribute nothing — they're skipped in the fold.
pub(in super::super) fn level_versions(
    ctx: &TreeCtx,
    seeds: &[NodeSeed],
) -> BTreeMap<String, Vec<String>> {
    let packages = lock_recoverable(&ctx.workspace.packages);
    let mut level: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for seed in seeds {
        let name_ver = match seed {
            NodeSeed::Pending(pending) => pending.result.name_ver.as_ref(),
            NodeSeed::Done(Some(dep)) => {
                packages.get(dep.id.as_str()).and_then(|pkg| pkg.result.name_ver.as_ref())
            }
            NodeSeed::Done(None) => None,
        };
        let Some(name_ver) = name_ver else { continue };
        let versions = level.entry(name_ver.name.to_string()).or_default();
        let version = name_ver.suffix.to_string();
        if !versions.contains(&version) {
            versions.push(version);
        }
    }
    level
}

/// Map an ancestor-id chain to the `parents` payload of a
/// skipped-optional-dependency notification, resolving each
/// `pkgIdWithPatchHash` through the shared packages map (the
/// counterpart of pnpm's `getPkgsInfoFromIds`).
pub(super) fn pkgs_info_from_ids(
    ctx: &TreeCtx,
    ancestor_ids: &[String],
) -> Vec<SkippedOptionalDependencyParent> {
    let packages = lock_recoverable(&ctx.workspace.packages);
    ancestor_ids
        .iter()
        .map(|id| {
            let name_ver = packages.get(id.as_str()).and_then(|pkg| pkg.result.name_ver.as_ref());
            SkippedOptionalDependencyParent {
                id: id.clone(),
                name: name_ver.map(|name_ver| name_ver.name.to_string()).unwrap_or_default(),
                version: name_ver.map(|name_ver| name_ver.suffix.to_string()).unwrap_or_default(),
            }
        })
        .collect()
}
