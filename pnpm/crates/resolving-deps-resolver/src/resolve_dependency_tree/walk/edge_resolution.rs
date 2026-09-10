use super::{
    Arc, BTreeMap, ChildEdge, Cow, NodeId, NodeSeed, PendingNode, PkgNameVerPeer,
    ResolveDependencyTreeError, ResolveOptions, ResolvedPackage, Resolver, SeededPackage,
    SkippedOptionalDependency, TreeCtx, UpdateBehavior, Value, WantedDependency, WantedKey,
    async_recursion, build_pkg_id_with_patch_hash, catalogs_for_children,
    current_pkg_from_lockfile, emit_deprecation_if_needed, ensure_same_registry_revision,
    extract_peer_dependencies, is_exotic_resolved_via, is_update_target, lock_recoverable,
    node_alias, node_depends_on_changed_direct_dep, opts_relative_to_declaring_manifest,
    overlay_version_view, parent_ids_contain_sequence, peer_shadowed_dependencies,
    pin_locked_version, pin_patched_revision, pkg_is_leaf, pkgs_info_from_ids,
    project_relative_cache_scope, register_peer_dep_names, resolve_reused_node,
    resolve_wanted_cached, resolves_children_through_catalogs, try_reuse_node,
    wanted_lockfile_contains_satisfying_entry,
};

#[async_recursion]
pub(in super::super) async fn resolve_node_seed<'e, Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    edge: ChildEdge<'e>,
) -> Result<NodeSeed, ResolveDependencyTreeError>
where
    'e: 'async_recursion,
    Chain: Resolver + ?Sized,
{
    let current_is_optional = wanted.optional.unwrap_or(false) || edge.parent_optional;

    // The edge's recorded snapshot key in the prior lockfile, if any.
    // Feeds both subtree reuse (below) and — when the edge re-resolves
    // anyway — the `currentPkg` payload custom resolvers receive.
    let prior_key = edge.reuse.prior_key(ctx, &wanted);

    // **Lockfile-resolution reuse.** When the prior lockfile already
    // resolved this edge (and the recorded version still satisfies the
    // manifest range, for a direct dep), synthesize the resolution from
    // the lockfile and walk its transitive subtree from the snapshot
    // graph instead of re-resolving from the registry.
    // `synthesize_reused_result` is conservative: any shape it can't
    // faithfully reproduce (non-registry resolutions, missing metadata)
    // yields `None` here and the node falls through to a fresh resolve.
    //
    // Stale-pin refresh: a node depending on a changed direct dep is
    // resolved fresh rather than reused, so its children walk against
    // their manifest ranges where `seed_node_children` can redirect a
    // stale pin onto the higher direct-dep version (reusing the subtree
    // would keep the pin, leaving the lockfile non-convergent).
    if ctx.workspace.reuse_lockfile_subtrees
        && edge.reuse.allows_reuse()
        && !node_depends_on_changed_direct_dep(ctx, prior_key.as_ref())
        && let Some(reused) = try_reuse_node(ctx, &wanted, prior_key.as_ref(), edge.depth)
    {
        return resolve_reused_node(ctx, resolver, wanted, &edge, current_is_optional, reused)
            .await
            .map(NodeSeed::Done);
    }

    // Locked-version pin, the fresh-resolve counterpart of subtree
    // reuse: a transitive edge whose recorded version still satisfies
    // its manifest range (`prior_key` is satisfies-gated) resolves to
    // exactly that version even when its subtree cannot be reused
    // wholesale. Without it, a re-resolve picks open ranges (`*`)
    // against the whole preferred-versions pool and lands every such
    // edge on the highest locked version, churning the lockfile.
    // Mirrors the TypeScript resolver's `replaceVersionInBareSpecifier`
    // under `!update`: direct deps (depth 0) keep recomputing their
    // specifier, and an edge a `pacquet update` reaches keeps
    // re-picking. Only plain semver ranges pin; aliased (`npm:`),
    // named-registry, and exotic specifiers keep today's behavior.
    let mut wanted = wanted;
    pin_locked_version(ctx, &mut wanted, prior_key.as_ref(), edge.depth);

    let Some(result) = resolve_edge(ctx, resolver, &mut wanted, &edge, prior_key.as_ref()).await?
    else {
        return Ok(NodeSeed::Done(None));
    };

    if let Some(violation) = result.policy_violation.clone() {
        lock_recoverable(&ctx.workspace.policy_violations).push(violation);
    }

    reject_exotic_subdep(ctx, &wanted, &result, edge.depth, edge.parent_is_workspace)?;

    let id = build_pkg_id_with_patch_hash(ctx, &result).await?;

    record_workspace_manifest_identity(ctx, &wanted, &result, &id);

    if closes_cycle(edge.ancestor_ids, &id) {
        return Ok(NodeSeed::Done(None));
    }

    seed_pending(ctx, &wanted, result, &edge, ResolvedEdge { id, prior_key, current_is_optional })
}

/// Memoise the per-wanted resolve. The first caller for a given
/// `(alias, bare_specifier, optional, injected)` runs the resolver chain
/// and stores the `Arc<ResolveResult>` on `ctx.resolved_by_wanted`;
/// every later caller for the same wanted dep clones the `Arc` and
/// skips the chain entirely. Concurrent first-callers can both miss the
/// cache and run `resolver.resolve` in parallel — the resolver's own
/// per-cache-key semaphore (`pick_package::fetch_locker`) already
/// coalesces those into a single network fetch, so the doubled work is
/// bounded to in-memory packument lookups + semver matching, and the
/// second to finish loses the `insert` race harmlessly (the entry holds
/// an `Arc` to an equivalent `ResolveResult`).
///
/// `None` when the edge is a droppable optional that failed to resolve.
pub(super) async fn resolve_edge<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: &mut WantedDependency,
    edge: &ChildEdge<'_>,
    prior_key: Option<&PkgNameVerPeer>,
) -> Result<Option<Arc<pnpm_resolving_resolver_base::ResolveResult>>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let base = edge_opts(ctx, wanted, edge, prior_key);
    let opts = opts_relative_to_declaring_manifest(&base, wanted, edge.parent_dir);
    let cache_key = edge_cache_key(ctx, wanted, &opts, edge, prior_key);
    match resolve_wanted_cached(ctx, resolver, wanted, &opts, edge.pick_overlay.as_ref(), cache_key)
        .await
    {
        Ok(result) => Ok(Some(result)),
        Err(err) => {
            drop_failed_optional_edge(ctx, wanted, edge.ancestor_ids, &opts, err)?;
            Ok(None)
        }
    }
}

/// `resolutionMode` makes the version pick depend on whether this is a
/// direct (`depth == 0`) or transitive dep, so the options key off the
/// depth. The prior lockfile entry rides along as `currentPkg`, handed
/// to the resolver. Only custom resolvers read it today; the clone of
/// the shared per-depth options is paid only when a prior entry exists
/// for a freshly resolving edge.
pub(super) fn edge_opts<'c>(
    ctx: &'c TreeCtx,
    wanted: &mut WantedDependency,
    edge: &ChildEdge<'_>,
    prior_key: Option<&PkgNameVerPeer>,
) -> Cow<'c, ResolveOptions> {
    let opts = ctx.opts_for_depth(edge.depth);
    let current_pkg = prior_key.and_then(|key| {
        let lockfile = ctx.workspace.wanted_lockfile.as_ref()?;
        current_pkg_from_lockfile(lockfile, key, &ctx.workspace.registry_context)
    });
    if opts.update == UpdateBehavior::Patches {
        pin_patched_revision(wanted, current_pkg.as_ref(), prior_key);
    }
    match current_pkg {
        Some(current_pkg) => {
            Cow::Owned(ResolveOptions { current_pkg: Some(current_pkg), ..opts.clone() })
        }
        None => Cow::Borrowed(opts),
    }
}

/// Project-relative resolutions (`link:`/`file:`/`workspace:`) are keyed
/// by the consuming importer so one importer's relative path is never
/// reused by another. See [`WantedKey`]. The prior key joins so two
/// edges that share a specifier but recorded different versions never
/// share a `currentPkg`-dependent result.
///
/// The overlay's view for this edge joins the cache key: the same range
/// can legitimately pick different versions under levels that resolved
/// different siblings. The view keeps each candidate name (alias, `npm:`
/// inner target, folded `jsr:` name) paired with its versions — the
/// picker consults the overlay per name, so a flat union of versions
/// could collide two overlays that distribute the same versions across
/// different names. Empty for almost every edge, so the dedup keeps
/// working where it matters.
pub(super) fn edge_cache_key(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    opts: &ResolveOptions,
    edge: &ChildEdge<'_>,
    prior_key: Option<&PkgNameVerPeer>,
) -> WantedKey {
    let project_scope = project_relative_cache_scope(wanted, opts);
    let overlay_versions = edge
        .pick_overlay
        .as_ref()
        .map(|overlay| overlay_version_view(overlay, wanted))
        .unwrap_or_default();
    let update_target = is_update_target(
        ctx.update_scope(),
        wanted,
        prior_key.and_then(|key| key.suffix.version_semver()),
        edge.depth,
    );
    WantedKey::new((
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.pick_lowest_version,
        opts.published_by,
        project_scope,
        prior_key.cloned(),
        overlay_versions,
        ctx.update_cache_scope(),
        update_target,
    ))
}

/// What a freshly resolved edge settled before its node seeds.
pub(super) struct ResolvedEdge {
    pub(super) id: String,
    pub(super) prior_key: Option<PkgNameVerPeer>,
    pub(super) current_is_optional: bool,
}

/// Build (or look up) the `ResolvedPackage` envelope. The first visitor
/// populates it; later visitors AND-fold the `optional` flag so a single
/// non-optional path flips it back to `false`. Child traversal is
/// claimed first, and a later deterministically-better occurrence
/// replaces the shared `children_by_id` entry — plus, since the two are
/// two halves of one manifest reading, the envelope's peer dependencies.
pub(super) fn seed_pending(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    result: Arc<pnpm_resolving_resolver_base::ResolveResult>,
    edge: &ChildEdge<'_>,
    resolved: ResolvedEdge,
) -> Result<NodeSeed, ResolveDependencyTreeError> {
    let alias = node_alias(wanted, &result, &resolved.id);
    let identity = NodeIdentity::of(&result, &resolved.id);
    let peer_shadowed = peer_shadowed_dependencies(
        result.manifest.as_deref(),
        edge.parent_pkg_aliases,
        ctx.workspace.auto_install_peers,
    );
    // The envelope's peer split follows the occurrence that owns the
    // package's children, which this level's settlement decides — see
    // [`fn@super::level_walk::install_owner_peer_dependencies`]. Seeding only has to fill
    // a package nothing has resolved yet.
    if register_seeded_package(
        ctx,
        SeededPackage {
            id: &resolved.id,
            result: &result,
            peer_shadowed: &peer_shadowed,
            resolves_children_through_catalogs: identity.resolves_children_through_catalogs,
            current_is_optional: resolved.current_is_optional,
            is_link: identity.is_link,
            is_leaf: identity.is_leaf,
        },
    )? {
        emit_deprecation_if_needed(ctx, &result, &resolved.id, edge.depth);
    }

    let next_ancestors: Vec<String> =
        edge.ancestor_ids.iter().cloned().chain(std::iter::once(resolved.id.clone())).collect();

    Ok(NodeSeed::Pending(Box::new(PendingNode {
        result,
        id: resolved.id,
        alias,
        node_id: identity.node_id,
        is_link: identity.is_link,
        resolves_children_through_catalogs: identity.resolves_children_through_catalogs,
        parent_ancestors: Arc::clone(edge.ancestor_ids),
        next_ancestors: Arc::new(next_ancestors),
        peer_shadowed,
        claim: None,
        depth: edge.depth,
        current_is_optional: resolved.current_is_optional,
        prior_key: resolved.prior_key,
    })))
}

/// Leaves (no deps / optional deps / peers / peerDependenciesMeta) reuse
/// the package id as their `NodeId`, collapsing every parent edge onto
/// one tree node. Non-leaves still get a fresh per-occurrence id so the
/// peer resolver can attach different peer suffixes per call site.
///
/// Workspace-link nodes get empty children (the linked project resolves
/// its own deps as a separate importer), `depth = -1` flags the node for
/// the peer-resolution short-circuit, and the [`ResolvedPackage`]
/// carries no peer dependencies (peer matching is the linked importer's
/// responsibility, not the parent's). The node id is collapsed to a leaf
/// so every reference to the same workspace path shares one [`NodeId`].
///
/// [`ResolvedPackage`]: crate::ResolvedPackage
pub(super) struct NodeIdentity {
    pub(super) is_link: bool,
    pub(super) resolves_children_through_catalogs: bool,
    /// Computed before the dedup insert so it can be persisted on
    /// [`ResolvedPackage::is_leaf`] for the lazy realisation path to
    /// read back.
    ///
    /// [`ResolvedPackage::is_leaf`]: crate::ResolvedPackage::is_leaf
    pub(super) is_leaf: bool,
    pub(super) node_id: NodeId,
}

impl NodeIdentity {
    pub(super) fn of(result: &Arc<pnpm_resolving_resolver_base::ResolveResult>, id: &str) -> Self {
        let is_link = id.starts_with("link:");
        let is_leaf = is_link || pkg_is_leaf(result);
        Self {
            is_link,
            resolves_children_through_catalogs: resolves_children_through_catalogs(result),
            is_leaf,
            node_id: node_id_for(is_leaf, id),
        }
    }
}

/// Leaves (no deps / optional deps / peers / `peerDependenciesMeta`) reuse the
/// package id as their `NodeId`, collapsing every parent edge onto one tree
/// node. Non-leaves get a fresh per-occurrence id so the peer resolver can
/// attach different peer suffixes per call site.
pub(in super::super) fn node_id_for(is_leaf: bool, id: &str) -> NodeId {
    if is_leaf { NodeId::leaf(id) } else { NodeId::next() }
}

/// Cycle break: a direct self-edge and the second lap of a longer cycle are
/// dropped; the first re-entry is kept so the cycle-closing edge reaches the
/// lockfile snapshot.
pub(in super::super) fn closes_cycle(ancestor_ids: &Arc<Vec<String>>, id: &str) -> bool {
    ancestor_ids
        .last()
        .is_some_and(|parent| parent == id || parent_ids_contain_sequence(ancestor_ids, parent, id))
}

/// Build (or look up) the [`ResolvedPackage`] envelope, answering whether this
/// occurrence is the one that created it. The first visitor populates it;
/// later visitors AND-fold the `optional` flag so a single non-optional path
/// flips it back to `false`.
///
/// The envelope's peer split follows the occurrence that owns the package's
/// children, which this level's settlement decides — see
/// [`fn@super::level_walk::install_owner_peer_dependencies`]. Seeding only has to fill a package
/// nothing has resolved yet.
pub(super) fn register_seeded_package(
    ctx: &TreeCtx,
    seeded: SeededPackage<'_>,
) -> Result<bool, ResolveDependencyTreeError> {
    let SeededPackage {
        id,
        result,
        peer_shadowed,
        resolves_children_through_catalogs,
        current_is_optional,
        is_link,
        is_leaf,
    } = seeded;
    let mut packages = lock_recoverable(&ctx.workspace.packages);
    if let Some(existing) = packages.get_mut(id) {
        ensure_same_registry_revision(existing, result)?;
        existing.optional = existing.optional && current_is_optional;
        return Ok(false);
    }
    // A workspace-link node carries no peer dependencies: peer matching is the
    // linked importer's responsibility, not the parent's.
    let peer_dependencies = if is_link {
        BTreeMap::new()
    } else {
        extract_peer_dependencies(
            result,
            peer_shadowed,
            catalogs_for_children(ctx, resolves_children_through_catalogs),
        )?
    };
    register_peer_dep_names(ctx, &peer_dependencies);
    ctx.workspace.record_package_write(id);
    let shared_id: Arc<str> = Arc::from(id);
    packages.insert(
        Arc::<str>::clone(&shared_id),
        ResolvedPackage {
            id: shared_id,
            result: Arc::clone(result),
            peer_dependencies,
            optional: current_is_optional,
            is_leaf,
        },
    );
    Ok(true)
}

/// A `workspace:` edge the resolver did not name carries its identity in the
/// linked project's own manifest.
pub(super) fn record_workspace_manifest_identity(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    id: &str,
) {
    if result.name_ver.is_some() {
        return;
    }
    let names_a_workspace_project = wanted.bare_specifier.as_deref().is_some_and(|specifier| {
        specifier.starts_with("workspace:") && !specifier.starts_with("workspace:.")
    });
    if !names_a_workspace_project {
        return;
    }
    let Some(manifest) = result.manifest.as_deref() else { return };
    let (Some(name), Some(version)) = (
        manifest.get("name").and_then(Value::as_str),
        manifest.get("version").and_then(Value::as_str),
    ) else {
        return;
    };
    ctx.workspace.record_workspace_manifest_identity(id, name, version);
}

pub(super) fn reject_exotic_subdep(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    depth: i32,
    parent_is_workspace: bool,
) -> Result<(), ResolveDependencyTreeError> {
    if !ctx.base_opts.block_exotic_subdeps
        || depth == 0
        || parent_is_workspace
        || !is_exotic_resolved_via(&result.resolved_via)
    {
        return Ok(());
    }
    Err(ResolveDependencyTreeError::ExoticSubdep {
        specifier: wanted
            .alias
            .clone()
            .or_else(|| wanted.bare_specifier.clone())
            .unwrap_or_default(),
        resolved_via: result.resolved_via.clone(),
    })
}

/// A resolution failure on an optional edge drops the edge instead of failing
/// the install — unless the wanted lockfile already holds a satisfying entry,
/// where the silent skip would erase the locked entries (see
/// [`fn@wanted_lockfile_contains_satisfying_entry`]). `Ok(())` means the edge
/// is dropped; every other failure propagates.
pub(super) fn drop_failed_optional_edge(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    ancestor_ids: &Arc<Vec<String>>,
    opts: &ResolveOptions,
    err: ResolveDependencyTreeError,
) -> Result<(), ResolveDependencyTreeError> {
    if !wanted.optional.unwrap_or(false) || !is_droppable_resolve_error(&err) {
        return Err(err);
    }
    if wanted_lockfile_contains_satisfying_entry(ctx.workspace.wanted_lockfile.as_deref(), wanted) {
        return Err(ResolveDependencyTreeError::LockedOptionalResolutionFailure(Box::new(err)));
    }
    if let Some(log) = ctx.workspace.skipped_optional_log.as_ref() {
        log(SkippedOptionalDependency {
            details: err.to_string(),
            name: wanted.alias.clone(),
            version: wanted.alias.is_some().then(|| wanted.bare_specifier.clone()).flatten(),
            bare_specifier: wanted.bare_specifier.clone().unwrap_or_default(),
            parents: pkgs_info_from_ids(ctx, ancestor_ids),
            prefix: opts.project_dir.display().to_string(),
        });
    }
    Ok(())
}

/// Hook errors keep aborting even for optional edges.
pub(super) fn is_droppable_resolve_error(err: &ResolveDependencyTreeError) -> bool {
    matches!(
        err,
        ResolveDependencyTreeError::Resolve(_)
            | ResolveDependencyTreeError::NoMatchingVersion(_)
            | ResolveDependencyTreeError::RegistryResponse(_)
            | ResolveDependencyTreeError::GitResolve(_)
            | ResolveDependencyTreeError::SpecNotSupported { .. },
    )
}
