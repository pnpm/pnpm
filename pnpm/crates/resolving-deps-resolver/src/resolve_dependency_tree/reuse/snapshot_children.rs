use super::{
    Arc, BTreeMap, ChildrenOwnerClaim, Cow, DirectDep, HashMap, HashSet, NodeId, ParentPkgAliases,
    PeerDep, Pipe, PkgName, PkgNameVerPeer, RecordedChildrenContext, ResolveDependencyTreeError,
    Resolver, ReuseSource, SnapshotDepRef, SnapshotEntry, TreeCtx, UpdateReuseScope,
    WantedDependency, future, is_current_children_owner, lazy_children, record_children,
    resolve_node,
};

/// The per-node context [`reused_children`] walks one reused node's snapshot
/// children against.
pub(super) struct ReusedChildren<'a> {
    pub(super) id: &'a str,
    pub(super) key: &'a PkgNameVerPeer,
    pub(super) snapshot: Option<&'a SnapshotEntry>,
    pub(super) child_refs: &'a [(String, PkgNameVerPeer)],
    pub(super) ancestor_ids: &'a Arc<Vec<String>>,
    pub(super) next_ancestors: &'a Arc<Vec<String>>,
    pub(super) depth: i32,
    pub(super) current_is_optional: bool,
    pub(super) parent_pkg_aliases: &'a Arc<ParentPkgAliases>,
}

/// Walk a reused node's children and record them, unless another occurrence
/// owns them — then this node's children stay lazy.
pub(super) async fn reused_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    claim: &ChildrenOwnerClaim,
    context: ReusedChildren<'_>,
) -> Result<(crate::resolved_tree::TreeChildren, bool), ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    if !claim.owns_children {
        return Ok((lazy_children(context.ancestor_ids), false));
    }
    let child_results = resolve_snapshot_children(ctx, resolver, &context).await?;
    if !is_current_children_owner(ctx, context.id, &claim.owner) {
        return Ok((lazy_children(context.ancestor_ids), false));
    }
    Ok(record_reused_children(ctx, claim, &context, child_results))
}

pub(super) async fn resolve_snapshot_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    context: &ReusedChildren<'_>,
) -> Result<Vec<Option<DirectDep>>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    context
        .child_refs
        .iter()
        .map(|(child_alias, child_key)| {
            let child_wanted = WantedDependency {
                alias: Some(child_alias.clone()),
                // The snapshot pins the exact version; carry it as
                // the bare specifier so the per-wanted dedup cache
                // key is stable and a fresh fallback (if reuse were
                // ever disabled) would still target the right pin.
                bare_specifier: Some(child_key.suffix.without_peer().to_string()),
                ..WantedDependency::default()
            };
            let next_ancestors = Arc::clone(context.next_ancestors);
            let child_key = child_key.clone();
            async move {
                resolve_node(
                    ctx,
                    resolver,
                    child_wanted,
                    &next_ancestors,
                    context.depth + 1,
                    context.current_is_optional,
                    ReuseSource::Transitive { key: Some(child_key) },
                    context.parent_pkg_aliases,
                )
                .await
            }
        })
        .pipe(future::try_join_all)
        .await
}

pub(super) fn record_reused_children(
    ctx: &TreeCtx,
    claim: &ChildrenOwnerClaim,
    context: &ReusedChildren<'_>,
    child_results: Vec<Option<DirectDep>>,
) -> (crate::resolved_tree::TreeChildren, bool) {
    let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
    let optional_by_alias: HashMap<&str, bool> = context
        .child_refs
        .iter()
        .map(|(alias, _)| (alias.as_str(), is_optional_child(context.snapshot, alias)))
        .collect();
    for dep in child_results.into_iter().flatten() {
        let optional = optional_by_alias.get(dep.alias.as_str()).copied().unwrap_or(false);
        by_id.push(crate::resolved_tree::ChildEdge {
            alias: dep.alias.clone(),
            pkg_id: Arc::from(dep.id),
            optional,
        });
        realized.insert(dep.alias, dep.node_id);
    }
    let recording = record_children(
        ctx,
        context.id,
        &claim.owner,
        by_id,
        RecordedChildrenContext {
            peer_shadowed: Arc::clone(&claim.peer_shadowed),
            prior_key: Some(context.key.clone()),
            update_active: !matches!(ctx.update_reuse_scope(), UpdateReuseScope::All),
        },
    );
    recording.into_children(realized, context.ancestor_ids)
}

/// `(install_alias, resolved_snapshot_key)` for every non-`link:` child
/// recorded on `snapshot`'s `dependencies` + `optionalDependencies`,
/// excluding resolved peers. Sorted by alias so the per-occurrence walk
/// order is deterministic.
///
/// A snapshot's `dependencies` map lists not only the package's real
/// dependencies but also every *resolved peer* — the node's own peers
/// (`peer_dependencies`) and the peers its descendants required and
/// resolved through this node (`transitivePeerDependencies`) — each
/// pinned to the version it matched in the recorded context. Those are
/// not real children: a fresh resolve walks only the package's manifest
/// `dependencies` and re-derives peers separately against the parent
/// context. Reuse must walk the manifest's deps too — so peer-named
/// entries are dropped here, and the snapshot's `dependencies` is used
/// only as the locked-ref lookup, not as the child set. Treating a
/// resolved peer as a regular child makes the peer pass satisfy the peer
/// from the node's own subtree instead of propagating it up, collapsing
/// the peer-context suffix.
pub(super) fn snapshot_child_refs(
    snapshot: Option<&SnapshotEntry>,
    peer_dependencies: &BTreeMap<String, PeerDep>,
) -> Vec<(String, PkgNameVerPeer)> {
    let Some(snapshot) = snapshot else { return Vec::new() };
    let transitive_peers: HashSet<&str> =
        snapshot.transitive_peer_dependencies.iter().flatten().map(String::as_str).collect();
    let mut out: Vec<(String, PkgNameVerPeer)> = Vec::new();
    for dep_map in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
    {
        push_snapshot_child_refs(dep_map, peer_dependencies, &transitive_peers, &mut out);
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub(super) fn push_snapshot_child_refs(
    dep_map: &std::collections::HashMap<PkgName, SnapshotDepRef>,
    peer_dependencies: &BTreeMap<String, PeerDep>,
    transitive_peers: &HashSet<&str>,
    out: &mut Vec<(String, PkgNameVerPeer)>,
) {
    for (alias, dep_ref) in dep_map {
        let alias_str = match &alias.scope {
            Some(scope) => Cow::Owned(format!("@{scope}/{}", alias.bare)),
            None => Cow::Borrowed(alias.bare.as_str()),
        };
        if peer_dependencies.contains_key(alias_str.as_ref())
            || transitive_peers.contains(alias_str.as_ref())
        {
            continue;
        }
        if let Some(key) = dep_ref.resolve(alias) {
            out.push((alias_str.into_owned(), key));
        }
    }
}

/// `true` when `alias` is recorded under `snapshot.optionalDependencies`
/// (as opposed to `dependencies`). Threads the right `optional` flag onto
/// the reused child's [`crate::resolved_tree::ChildEdge`].
pub(super) fn is_optional_child(snapshot: Option<&SnapshotEntry>, alias: &str) -> bool {
    let Some(snapshot) = snapshot else { return false };
    let Ok(name) = alias.parse::<pnpm_lockfile::PkgName>() else { return false };
    snapshot.optional_dependencies.as_ref().is_some_and(|deps| deps.contains_key(&name))
}
