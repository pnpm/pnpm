use super::{
    ChildSpec, HashSet, NodeSeed, ParentPkgAliases, Pipe, ResolveOptions, Resolver, TreeCtx,
    WantedDependency, WantedKey, async_recursion, catalogs_for_children, claim_children_warmup,
    declaring_manifest_dir, extract_children, future, is_update_target,
    opts_relative_to_declaring_manifest, peer_shadowed_dependencies, project_relative_cache_scope,
    resolve_catalog_child_specs, resolve_wanted_cached, resolves_children_through_catalogs,
};

/// Speculatively warm a freshly-seeded node's whole subtree so its
/// packuments download while the level barriers wait for their
/// slowest members. Results are discarded — the real picks run in the
/// walk phase with the level's preferred-versions overlay and hit the
/// warm metadata caches — and errors are swallowed: a speculative
/// fetch must never fail the install (the real resolve will surface
/// it). Recovers the cross-level pipelining the postponed-resolution
/// barrier otherwise serializes; pure overlap, no behavioral effect.
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
    if ctx.workspace.pnpmfile_hook.is_some() {
        return;
    }
    let NodeSeed::Pending(pending) = seed else { return };
    if pending.is_link || !claim_children_warmup(ctx, &pending.id) {
        return;
    }
    warm_result_children(
        ctx,
        resolver,
        &pending.result,
        &pending.peer_shadowed,
        pending.resolves_children_through_catalogs,
        pending.depth,
    )
    .await;
}

/// Warm the resolutions of `result`'s whole subtree. Speculative only:
/// nothing is recorded in the tree, every package is visited at most
/// once across the walk, and a child whose own peers shadow it is
/// skipped the way the real seed path skips it.
#[async_recursion]
pub(super) async fn warm_result_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    peer_shadowed: &HashSet<String>,
    through_catalogs: bool,
    depth: i32,
) where
    Chain: Resolver + ?Sized,
{
    let Some(specs) = warm_child_specs(ctx, result, peer_shadowed, through_catalogs) else {
        return;
    };
    let opts = ctx.opts_for_depth(depth + 1);
    let declaring_dir = declaring_manifest_dir(ctx, result);
    specs
        .iter()
        .map(|(name, range, optional, injected)| {
            let wanted = WantedDependency {
                alias: Some(name.clone()),
                bare_specifier: Some(range.clone()),
                optional: Some(*optional),
                injected: injected.then_some(true),
                ..WantedDependency::default()
            };
            let opts = opts_relative_to_declaring_manifest(opts, &wanted, declaring_dir.as_deref());
            async move { warm_child(ctx, resolver, wanted, opts.as_ref(), depth).await }
        })
        .pipe(future::join_all)
        .await;
}

/// The child edges a warm-up walks: the declared children minus the ones a
/// peer shadows, with `catalog:` ranges dereferenced. `None` when the
/// manifest or a catalog reference cannot be read — the real walk reports it.
pub(super) fn warm_child_specs(
    ctx: &TreeCtx,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    peer_shadowed: &HashSet<String>,
    through_catalogs: bool,
) -> Option<Vec<ChildSpec>> {
    let specs = extract_children(result).ok()?;
    let specs = if peer_shadowed.is_empty() {
        specs
    } else {
        specs
            .into_iter()
            .filter(|(name, _, optional, _)| *optional || !peer_shadowed.contains(name))
            .collect()
    };
    let Some(catalogs) = catalogs_for_children(ctx, through_catalogs) else { return Some(specs) };
    resolve_catalog_child_specs(specs, catalogs).ok()
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
) where
    Chain: Resolver + ?Sized,
{
    let project_scope = project_relative_cache_scope(&wanted, opts);
    let cache_key = WantedKey::new((
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.pick_lowest_version,
        opts.published_by,
        project_scope,
        // No prior-lockfile key: a warm entry must only be
        // reused by edges that carry no currentPkg either.
        None,
        Vec::new(),
        ctx.update_cache_scope(),
        is_update_target(ctx.update_scope(), &wanted, None, parent_depth + 1),
    ));
    let Ok(child) = resolve_wanted_cached(ctx, resolver, &wanted, opts, None, cache_key).await
    else {
        return;
    };
    // Claimed by the resolver's raw id: the patch-qualified
    // id is only built on the real walk, whose bookkeeping
    // decides which patches count as applied.
    let child_id = child.id.as_str();
    if child_id.starts_with("link:") || !claim_children_warmup(ctx, child_id) {
        return;
    }
    // The parent alias scope is not tracked speculatively;
    // with `autoInstallPeers` off nothing is shadowed and
    // the real walk drops the edge instead.
    let child_peer_shadowed = peer_shadowed_dependencies(
        child.manifest.as_deref(),
        &ParentPkgAliases::root(HashSet::default()),
        ctx.workspace.auto_install_peers,
    );
    warm_result_children(
        ctx,
        resolver,
        &child,
        &child_peer_shadowed,
        resolves_children_through_catalogs(&child),
        parent_depth + 1,
    )
    .await;
}
