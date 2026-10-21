use super::{
    ChildSeedScope, DependencySpec, HashSet, NodeKind, NodeSeed, ParentPkgAliases, Pipe,
    PkgNameVerPeer, ResolveOptions, Resolver, TreeCtx, WantedDependency, WantedKey,
    async_recursion, child_wanted, children_warmup_claimed, claim_children_warmup,
    declaring_manifest_dir, extract_children, future, is_update_target, landed_on_prior_entry,
    node_depends_on_changed_direct_dep, opts_relative_to_declaring_manifest,
    peer_shadowed_dependencies, project_relative_cache_scope, resolve_wanted_cached,
    resolves_children_through_catalogs, try_reuse_node, walk_child_specs, wanted_package_root_link,
};

/// Speculatively warm a freshly-seeded node's whole subtree so its
/// packuments download while the level barriers wait for their
/// slowest members. Results are discarded — the real picks run in the
/// walk phase with the level's preferred-versions overlay and hit the
/// warm metadata caches — and errors are swallowed: a speculative
/// fetch must never fail the install (the real resolve will surface
/// it). Recovers the cross-level pipelining the postponed-resolution
/// barrier otherwise serializes; pure overlap, no behavioral effect.
///
/// A child the walk will take from the prior lockfile's subtree is left
/// alone: the walk never consults metadata for it, so warming it would
/// only cost the lookups. The children's lockfile keys are derived as
/// the walk derives them, from the snapshot the node landed on. Whether
/// a child is reused is decided per importer (its update scope, its
/// changed direct dependencies), so a package is claimed as warmed only
/// when every child was warmed: another importer that must resolve a
/// child skipped here warms it when it reaches the package.
pub(in super::super) async fn warm_children_resolutions<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    seed: &NodeSeed,
) where
    Chain: Resolver + ?Sized,
{
    // A configured pnpmfile hook is externally observable per call
    // (`readPackage` IPC, `context.log`, custom resolvers), so
    // speculative resolutions must not fire it; the pure in-memory
    // manifest hook (packageExtensions / overrides) is idempotent and
    // cache-deduped, indistinguishable from a first-caller win in the
    // pre-existing concurrent-miss race.
    if ctx.workspace.hooks.manifests.pnpmfile_hook.is_some() {
        return;
    }
    let NodeSeed::Pending(pending) = seed else { return };
    match pending.kind {
        NodeKind::Link => return,
        NodeKind::Leaf | NodeKind::Branch => {}
    }
    let parent = WarmedParent {
        result: &pending.result,
        peer_shadowed: &pending.peer_shadowed,
        through_catalogs: pending.resolves_children_through_catalogs,
        claim_id: &pending.identity.id,
    };
    warm_result_children(
        ctx,
        resolver,
        &parent,
        pending.ancestry.depth,
        &ChildSeedScope::of(ctx, pending),
    )
    .await;
}

/// A node whose children are warmed.
pub(super) struct WarmedParent<'a> {
    pub(super) result: &'a pnpm_resolving_resolver_base::ResolveResult,
    /// Children shadowed by the node's own peers, skipped the way the
    /// real seed path skips them.
    pub(super) peer_shadowed: &'a HashSet<String>,
    pub(super) through_catalogs: bool,
    /// The id the node's children are claimed under, once all of them
    /// were warmed.
    pub(super) claim_id: &'a str,
}

/// Whether the walk will synthesize this edge's subtree from the prior
/// lockfile instead of resolving it, under the gate `resolve_node_seed`
/// applies.
fn walk_reuses_subtree(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    prior: Option<&PkgNameVerPeer>,
    depth: i32,
) -> bool {
    ctx.workspace.reuse.subtrees
        && !node_depends_on_changed_direct_dep(ctx, prior)
        && try_reuse_node(ctx, wanted, prior, depth).is_some()
}

/// Warm the resolutions of `result`'s whole subtree. Speculative only:
/// nothing is recorded in the tree, every package is visited at most
/// once across the walk, and a child whose own peers shadow it is
/// skipped the way the real seed path skips it.
#[async_recursion]
pub(super) async fn warm_result_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    parent: &WarmedParent<'_>,
    depth: i32,
    scope: &ChildSeedScope<'_>,
) where
    Chain: Resolver + ?Sized,
{
    let importer_claim_id = importer_claim_id(ctx, parent.claim_id);
    if children_warmup_claimed(ctx, parent.claim_id)
        || children_warmup_claimed(ctx, &importer_claim_id)
    {
        return;
    }
    let Some(specs) =
        warm_child_specs(ctx, parent.result, parent.peer_shadowed, parent.through_catalogs)
    else {
        return;
    };
    let (warmed, skipped_any) = children_to_warm(ctx, scope, &specs, depth + 1);
    // A package some of whose children this importer left to lockfile
    // reuse is claimed for this importer only: another importer, with
    // another update scope, may have to warm them. The claim still stops
    // a dependency cycle from being warmed again and again.
    let claim_id = if skipped_any { importer_claim_id.as_str() } else { parent.claim_id };
    if !claim_children_warmup(ctx, claim_id) {
        return;
    }
    let opts = ctx.opts_for_depth(depth + 1);
    let declaring_dir = declaring_manifest_dir(ctx, parent.result);
    warmed
        .into_iter()
        .map(|(wanted, prior)| {
            let opts = opts_relative_to_declaring_manifest(opts, &wanted, declaring_dir.as_deref());
            async move { warm_child(ctx, resolver, wanted, opts.as_ref(), depth, prior).await }
        })
        .pipe(future::join_all)
        .await;
}

/// The key a package's warm-up is claimed under for the current importer
/// alone. A package id never contains a tab, so the key cannot collide
/// with a plain id.
fn importer_claim_id(ctx: &TreeCtx, pkg_id: &str) -> String {
    format!("{}\t{pkg_id}", ctx.importer.id)
}

/// The children of a node to warm, with their lockfile keys, and whether
/// any child was left to the walk's lockfile reuse.
fn children_to_warm(
    ctx: &TreeCtx,
    scope: &ChildSeedScope<'_>,
    specs: &[DependencySpec],
    depth: i32,
) -> (Vec<(WantedDependency, Option<PkgNameVerPeer>)>, bool) {
    let mut skipped_any = false;
    let warmed = specs
        .iter()
        .filter_map(|spec| {
            let (wanted, prior) = child_wanted(ctx, scope, spec, depth);
            if walk_reuses_subtree(ctx, &wanted, prior.as_ref(), depth) {
                skipped_any = true;
                return None;
            }
            Some((wanted, prior))
        })
        .collect();
    (warmed, skipped_any)
}

/// The child edges a warm-up walks: the declared children minus the ones a
/// peer shadows, with `catalog:` ranges dereferenced. `None` when the
/// manifest or a catalog reference cannot be read — the real walk reports it.
pub(super) fn warm_child_specs(
    ctx: &TreeCtx,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    peer_shadowed: &HashSet<String>,
    through_catalogs: bool,
) -> Option<Vec<DependencySpec>> {
    let specs = extract_children(result).ok()?;
    walk_child_specs(ctx, result, &specs, peer_shadowed, through_catalogs).ok()
}

/// Warm one child edge through the same per-wanted dedup cache, under the
/// empty-overlay-view key: when the real pick's view is empty too (the
/// overwhelmingly common case) it reuses this entry outright; otherwise it
/// misses into its own bucket and re-picks from the warm metadata caches.
pub(super) async fn warm_child<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    opts: &ResolveOptions,
    parent_depth: i32,
    prior: Option<PkgNameVerPeer>,
) where
    Chain: Resolver + ?Sized,
{
    // A link into the declaring package is never resolved; see
    // `package_root_link_result`.
    if wanted_package_root_link(&wanted).is_some() {
        return;
    }
    let project_scope = project_relative_cache_scope(&wanted, opts);
    let cache_key = WantedKey::new((
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.version.pick_lowest_version,
        (opts.policy.published_by, opts.policy.fallback_published_by),
        project_scope,
        // No prior-lockfile key: a warm entry must only be
        // reused by edges that carry no currentPkg either.
        None,
        Vec::new(),
        ctx.update_cache_scope(),
        is_update_target(ctx.update_scope(), &wanted, None, parent_depth + 1),
        opts.refresh.prefer_current_version,
    ));
    let Ok(child) = resolve_wanted_cached(ctx, resolver, &wanted, opts, None, cache_key).await
    else {
        return;
    };
    // Claimed by the resolver's raw id: the patch-qualified
    // id is only built on the real walk, whose bookkeeping
    // decides which patches count as applied.
    let child_id = child.id.as_str();
    if child_id.starts_with("link:") {
        return;
    }
    let landed_prior = prior.as_ref().filter(|key| landed_on_prior_entry(key, child_id));
    warm_grandchildren(ctx, resolver, &child, landed_prior, parent_depth + 1).await;
}

/// Warm the children of a warmed `child`, whose own children the walk
/// derives from the lockfile entry `landed_prior` when it landed on one.
async fn warm_grandchildren<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    child: &pnpm_resolving_resolver_base::ResolveResult,
    landed_prior: Option<&PkgNameVerPeer>,
    depth: i32,
) where
    Chain: Resolver + ?Sized,
{
    // The parent alias scope is not tracked speculatively;
    // with `autoInstallPeers` off nothing is shadowed and
    // the real walk drops the edge instead.
    let child_peer_shadowed = peer_shadowed_dependencies(
        child.package.manifest.as_deref(),
        &ParentPkgAliases::root(HashSet::default()),
        ctx.workspace.policy.auto_install_peers,
    );
    let parent = WarmedParent {
        result: child,
        peer_shadowed: &child_peer_shadowed,
        through_catalogs: resolves_children_through_catalogs(child),
        claim_id: child.id.as_str(),
    };
    warm_result_children(
        ctx,
        resolver,
        &parent,
        depth,
        &ChildSeedScope::for_result(ctx, child, landed_prior),
    )
    .await;
}
