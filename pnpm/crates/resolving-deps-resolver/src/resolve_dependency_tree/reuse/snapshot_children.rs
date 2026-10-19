use futures_util::future;
use pipe_trait::Pipe;

use super::{
    AncestorPkgIds, Arc, BTreeMap, BoundProviders, ChildEdge, ChildrenOwnerClaim,
    ChildrenRecording, Cow, DirectDep, HashSet, NodeId, NodeSeed, ParentPkgAliases, PeerDep,
    PeerSet, PkgName, PkgNameVerPeer, RecordedChildrenContext, ResolveDependencyTreeError,
    Resolver, ReuseSource, ReusedNode, SnapshotDepRef, SnapshotEntry, TreeChildren, TreeCtx,
    UpdateReuseScope, WantedDependency, declared_peer_dependencies, is_current_children_owner,
    level_aliases, node_id_for, record_children, register_shadowable_dep_names, resolve_node_seed,
    walk_from_seeds,
};

/// A reused node whose package has been registered on the workspace tree
/// and whose snapshot children are waiting on the level's settled
/// [`ParentPkgAliases`] scope.
pub(in super::super) struct PendingReusedNode {
    pub(in super::super) dep: DirectDep,
    pub(super) key: PkgNameVerPeer,
    pub(super) child_refs: Vec<(String, PkgNameVerPeer, bool)>,
    pub(super) peer_set: PeerSet,
    pub(super) peer_shadowed: HashSet<String>,
    pub(super) ancestry: ReusedNodeAncestry,
}

pub(super) struct ReusedNodeAncestry {
    pub(super) parent_ancestors: AncestorPkgIds,
    pub(super) next_ancestors: AncestorPkgIds,
    pub(super) depth: i32,
    pub(super) current_is_optional: bool,
}

impl ReusedNodeAncestry {
    pub(super) fn new(edge: &ChildEdge<'_>, id: &Arc<str>, current_is_optional: bool) -> Self {
        Self {
            parent_ancestors: Arc::clone(edge.ancestor_ids),
            next_ancestors: Arc::new(
                edge.ancestor_ids
                    .iter()
                    .cloned()
                    .chain(std::iter::once(Arc::clone(id)))
                    .collect(),
            ),
            depth: edge.depth,
            current_is_optional,
        }
    }
}

/// Walk a reused node's children and record them, unless another occurrence
/// owns them — then this node's children stay lazy.
pub(super) async fn reused_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    claim: &ChildrenOwnerClaim,
    context: &PendingReusedNode,
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<(TreeChildren, ChildrenRecording), ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    if !claim.owns_children {
        return Ok((TreeChildren::Lazy, ChildrenRecording::Declined));
    }
    let recorded_context = RecordedChildrenContext {
        peer_shadowed: Arc::clone(&claim.peer_shadowed),
        prior_key: Some(context.key.clone()),
        reused: true,
        update_active: !matches!(ctx.update_reuse_scope(), UpdateReuseScope::All),
        children_scope: Arc::clone(children_pkg_aliases),
    };
    let child_results =
        resolve_snapshot_children(ctx, resolver, context, children_pkg_aliases).await?;
    if !is_current_children_owner(ctx, &context.dep.id, &claim.owner) {
        return Ok((TreeChildren::Lazy, ChildrenRecording::Declined));
    }
    Ok(record_reused_children(ctx, claim, context, recorded_context, child_results))
}

pub(super) async fn resolve_snapshot_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    context: &PendingReusedNode,
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<Vec<Option<DirectDep>>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let base_overlay = ctx.options.base.version.preferred_versions_overlay.clone();
    let seeds = seed_snapshot_children(ctx, resolver, context, children_pkg_aliases).await?;
    let grandchildren_pkg_aliases = children_pkg_aliases.extend(level_aliases(&seeds));
    seeds
        .into_iter()
        .map(|seed| {
            let base_overlay = base_overlay.clone();
            let grandchildren_pkg_aliases = Arc::clone(&grandchildren_pkg_aliases);
            async move {
                let direct = walk_from_seeds(
                    ctx,
                    resolver,
                    vec![seed],
                    base_overlay,
                    grandchildren_pkg_aliases,
                )
                .await?;
                Ok(direct.into_iter().next())
            }
        })
        .pipe(future::try_join_all)
        .await
}

async fn seed_snapshot_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    context: &PendingReusedNode,
    children_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<Vec<NodeSeed>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let base_overlay = &ctx.options.base.version.preferred_versions_overlay;
    context.child_refs
        .iter()
        .map(|(child_alias, child_key, _)| {
            let child_wanted = WantedDependency {
                alias: Some(child_alias.clone()),
                // The snapshot pins the exact version; carry it as
                // the bare specifier so the per-wanted dedup cache
                // key is stable.
                bare_specifier: Some(pinned_specifier(child_alias, child_key)),
                ..WantedDependency::default()
            };
            let next_ancestors = Arc::clone(&context.ancestry.next_ancestors);
            let child_key = child_key.clone();
            let pick_overlay = base_overlay.clone();
            async move {
                resolve_node_seed(
                    ctx,
                    resolver,
                    child_wanted,
                    ChildEdge {
                        ancestor_ids: &next_ancestors,
                        depth: context.ancestry.depth + 1,
                        parent_optional: context.ancestry.current_is_optional,
                        reuse: ReuseSource::Transitive { key: Some(child_key) },
                        pick_overlay,
                        parent_dir: None,
                        parent_pkg_aliases: children_pkg_aliases,
                        parent_is_workspace: false,
                    },
                )
                .await
            }
        })
        .pipe(future::try_join_all)
        .await
}

/// The specifier that resolves `alias` to exactly the locked `key`. An
/// aliased child (an `npm:` alias, often written by an override) names its
/// real package, so a child that falls through to a fresh resolve fetches
/// that package rather than a same-numbered version of `alias`.
pub(super) fn pinned_specifier(alias: &str, key: &PkgNameVerPeer) -> String {
    let version = key.suffix.without_peer();
    if key.name.to_string() == alias {
        return version.to_string();
    }
    if let Some((registry_name, version)) = key.suffix.registry_qualified() {
        return format!("{registry_name}:{}@{version}", key.name);
    }
    match key.suffix.version_semver() {
        Some(version) => format!("npm:{}@{version}", key.name),
        None => version.to_string(),
    }
}

pub(super) fn record_reused_children(
    ctx: &TreeCtx,
    claim: &ChildrenOwnerClaim,
    context: &PendingReusedNode,
    recorded_context: RecordedChildrenContext,
    child_results: Vec<Option<DirectDep>>,
) -> (TreeChildren, ChildrenRecording) {
    let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
    let optional_by_alias: super::HashMap<&str, bool> = context.child_refs
        .iter()
        .map(|(alias, _, optional)| (alias.as_str(), *optional))
        .collect();
    for dep in child_results.into_iter().flatten() {
        let optional = optional_by_alias
            .get(dep.alias.as_str())
            .copied()
            .unwrap_or(false);
        by_id.push(crate::resolved_tree::ChildEdge {
            alias: dep.alias.clone(),
            pkg_id: dep.id,
            optional,
        });
        realized.insert(dep.alias, dep.node_id);
    }
    let recording = record_children(ctx, &context.dep.id, &claim.owner, by_id, recorded_context);
    (recording.children(realized), recording)
}

/// Whether the peer-provider split recorded on `reused.key` matches what a
/// fresh resolve of `reused` under `scope` would produce.
///
/// A snapshot entry for a package that declares `peer` in both `dependencies`
/// and `peerDependencies` has `peer` in `snapshot.dependencies` either way:
/// when the peer was shadowed it holds the peer-resolved target (recorded in
/// the key's peer suffix), and when it was kept as an own dependency it holds
/// the own-dep child (absent from the peer suffix). If the current `scope`
/// disagrees on whether `peer` is shadowed, reusing the snapshot would treat
/// an own-dep child as a peer or vice versa.
pub(in super::super) fn reuse_matches_peer_scope(
    ctx: &TreeCtx,
    reused: &ReusedNode,
    scope: &ParentPkgAliases,
) -> bool {
    reuse_key_matches_peer_scope(ctx, reused.result.package.manifest.as_deref(), &reused.key, scope)
}

pub(in super::super) fn reuse_key_matches_peer_scope(
    ctx: &TreeCtx,
    manifest: Option<&serde_json::Value>,
    key: &PkgNameVerPeer,
    scope: &ParentPkgAliases,
) -> bool {
    let Some(manifest) = manifest else {
        return true;
    };
    let Some(snapshot) = ctx.workspace.reuse.lockfile
        .as_ref()
        .and_then(|lf| lf.snapshots.as_ref())
        .and_then(|s| s.get(key))
    else {
        return true;
    };
    let auto_install_peers = ctx.workspace.policy.auto_install_peers;
    let providers = BoundProviders::from_key(key);
    declared_peer_dependencies(manifest)
        .into_iter()
        .all(|(name, peer)| {
            let shadowed_in_scope = (auto_install_peers && !peer.optional) || scope.contains(&name);
            let Some(dep_ref) = snapshot_dep_ref(snapshot, &name) else {
                return true;
            };
            if providers.is_hashed() {
                return auto_install_peers && !peer.optional;
            }
            let Ok(pkg_name) = name.parse::<PkgName>() else { return false };
            let Some(target) = dep_ref.resolve(&pkg_name) else { return false };
            providers.binds(&target) == shadowed_in_scope
        })
}

pub(super) struct ReusedIdentity {
    pub(super) peer_set: PeerSet,
    pub(super) peer_shadowed: HashSet<String>,
    pub(super) child_refs: Vec<(String, PkgNameVerPeer, bool)>,
    pub(super) is_leaf: bool,
    pub(super) node_id: NodeId,
}

pub(super) fn reused_identity(
    ctx: &TreeCtx,
    id: &str,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    key: &PkgNameVerPeer,
) -> ReusedIdentity {
    let snapshot = ctx.workspace.reuse.lockfile
        .as_ref()
        .and_then(|lf| lf.snapshots.as_ref())
        .and_then(|s| s.get(key));
    let declared: BTreeMap<String, PeerDep> = result.package.manifest
        .as_deref()
        .map(declared_peer_dependencies)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let (own_deps, peer_shadowed) = split_reused_peers(ctx, key, snapshot, &declared);
    let peer_set = PeerSet::new(declared, |name| own_deps.contains(name));
    let child_refs = snapshot_child_refs(snapshot, peer_set.taken());
    let is_leaf = child_refs.is_empty() && peer_set.declared().is_empty();
    let node_id = node_id_for(is_leaf, id);
    ReusedIdentity { peer_set, peer_shadowed, child_refs, is_leaf, node_id }
}

fn split_reused_peers(
    ctx: &TreeCtx,
    key: &PkgNameVerPeer,
    snapshot: Option<&SnapshotEntry>,
    declared: &BTreeMap<String, PeerDep>,
) -> (HashSet<String>, HashSet<String>) {
    let Some(snapshot) = snapshot else {
        return (HashSet::default(), HashSet::default());
    };
    let auto_install_peers = ctx.workspace.policy.auto_install_peers;
    let providers = BoundProviders::from_key(key);
    let mut own_deps = HashSet::default();
    let mut peer_shadowed = HashSet::default();
    let mut shadowable = Vec::new();
    for (name, peer) in declared {
        let Some(is_bound) = classify_reused_peer(snapshot, &providers, name) else { continue };
        if !auto_install_peers || peer.optional {
            shadowable.push(name);
        }
        if is_bound {
            peer_shadowed.insert(name.clone());
        } else {
            own_deps.insert(name.clone());
        }
    }
    register_shadowable_dep_names(ctx, shadowable);
    (own_deps, peer_shadowed)
}

fn classify_reused_peer(
    snapshot: &SnapshotEntry,
    providers: &BoundProviders,
    name: &str,
) -> Option<bool> {
    let dep_ref = snapshot_dep_ref(snapshot, name)?;
    if providers.is_hashed() {
        return Some(true);
    }
    let pkg_name = name.parse::<PkgName>().ok()?;
    let target = dep_ref.resolve(&pkg_name)?;
    Some(providers.binds(&target))
}

fn snapshot_dep_ref<'a>(snapshot: &'a SnapshotEntry, name: &str) -> Option<&'a SnapshotDepRef> {
    let pkg_name = name.parse::<PkgName>().ok()?;
    snapshot.dependencies
        .as_ref()
        .and_then(|deps| deps.get(&pkg_name))
        .or_else(|| {
            snapshot.optional_dependencies
                .as_ref()
                .and_then(|deps| deps.get(&pkg_name))
        })
}

/// `(install_alias, resolved_snapshot_key, optional)` for every non-`link:`
/// child recorded on `snapshot`'s `dependencies` + `optionalDependencies`,
/// excluding the node's own resolved peers (`peer_dependencies`). Sorted by
/// alias so the per-occurrence walk order is deterministic.
///
/// A snapshot's `dependencies` map lists the package's real dependencies
/// overlaid with its own resolved peers (`peer_dependencies`), each pinned to
/// the version it matched in the recorded context. Those peer entries are not
/// real children: a fresh resolve walks only the package's manifest
/// `dependencies` and re-derives peers separately against the parent context.
/// Reuse must walk the manifest's deps too — so own-peer entries are dropped
/// here, and the snapshot's `dependencies` is used only as the locked-ref
/// lookup, not as the child set.
pub(super) fn snapshot_child_refs(
    snapshot: Option<&SnapshotEntry>,
    peer_dependencies: &BTreeMap<String, PeerDep>,
) -> Vec<(String, PkgNameVerPeer, bool)> {
    let Some(snapshot) = snapshot else { return Vec::new() };
    let mut out: Vec<(String, PkgNameVerPeer, bool)> = Vec::new();
    for (dep_map, optional) in
        [(snapshot.dependencies.as_ref(), false), (snapshot.optional_dependencies.as_ref(), true)]
    {
        if let Some(dep_map) = dep_map {
            push_snapshot_child_refs(dep_map, peer_dependencies, optional, &mut out);
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub(super) fn push_snapshot_child_refs(
    dep_map: &std::collections::HashMap<PkgName, SnapshotDepRef>,
    peer_dependencies: &BTreeMap<String, PeerDep>,
    optional: bool,
    out: &mut Vec<(String, PkgNameVerPeer, bool)>,
) {
    for (alias, dep_ref) in dep_map {
        let alias_str = match &alias.scope {
            Some(scope) => Cow::Owned(format!("@{scope}/{}", alias.bare)),
            None => Cow::Borrowed(alias.bare.as_str()),
        };
        if peer_dependencies.contains_key(alias_str.as_ref()) {
            continue;
        }
        if let Some(key) = dep_ref.resolve(alias) {
            out.push((alias_str.into_owned(), key, optional));
        }
    }
}
