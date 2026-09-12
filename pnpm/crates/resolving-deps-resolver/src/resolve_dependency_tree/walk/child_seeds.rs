use super::{
    Arc, Catalogs, ChildEdge, ChildSpec, Cow, FrontierNode, HashSet, NodeSeed, Path, PendingNode,
    Pipe, PkgNameVerPeer, PreferredVersionsOverlay, ResolveDependencyTreeError, Resolver,
    ReuseSource, SeededNode, SnapshotEntry, TreeCtx, WantedDependency, async_recursion,
    declaring_manifest_dir, extract_children, future, higher_direct_dep_version, level_aliases,
    level_versions, lock_recoverable, prior_child_key, real_package_name_of,
    resolve_catalog_specifier, resolve_node_seed, warm_children_resolutions,
};

/// Seed every child edge of one occurrence — its manifest's
/// dependencies less the names its own `peerDependencies` shadow, with
/// a workspace project's catalog specifiers resolved — and derive the
/// scope its grandchildren will resolve in.
///
/// The whole level seeds before any of it settles, so this resolves
/// packages and takes no ownership: [`fn@super::level_walk::assign_level_owners`] does
/// that once every occurrence of the level is in.
#[async_recursion]
pub(super) async fn seed_node_children<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    node: FrontierNode,
) -> Result<SeededNode, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let child_specs = child_specs_of(ctx, &node.pending, &node.claim.peer_shadowed)?;
    let scope = ChildSeedScope::of(ctx, &node.pending);
    let seeds = child_specs
        .iter()
        .map(|spec| seed_child(ctx, resolver, &node, &scope, spec))
        .pipe(future::try_join_all)
        .await?;
    let grandchild_overlay =
        PreferredVersionsOverlay::layer(node.children_overlay.clone(), level_versions(ctx, &seeds));
    let grandchild_pkg_aliases = node.children_pkg_aliases.extend(level_aliases(&seeds));
    Ok(SeededNode { node, child_specs, seeds, grandchild_overlay, grandchild_pkg_aliases })
}

/// The occurrence's child specs: its manifest's dependencies less the
/// names its own `peerDependencies` shadow, with a workspace project's
/// catalog specifiers resolved.
///
/// The manifest's specs are cached per package id. The cache value is
/// held by `Arc` so revisits clone the refcount instead of the inner
/// `Vec<ChildSpec>`, and it is cached unfiltered because which of the
/// specs the package's own `peerDependencies` shadow is a property of
/// the owner occurrence, not of the manifest.
pub(super) fn child_specs_of(
    ctx: &TreeCtx,
    pending: &PendingNode,
    peer_shadowed: &HashSet<String>,
) -> Result<Arc<Vec<ChildSpec>>, ResolveDependencyTreeError> {
    let cached =
        lock_recoverable(&ctx.workspace.children_specs_by_id).get(pending.id.as_str()).cloned();
    let child_specs = if let Some(specs) = cached {
        specs
    } else {
        let specs = Arc::new(extract_children(&pending.result)?);
        lock_recoverable(&ctx.workspace.children_specs_by_id)
            .entry(Arc::from(pending.id.as_str()))
            .or_insert_with(|| Arc::clone(&specs));
        specs
    };
    let child_specs = if peer_shadowed.is_empty() {
        child_specs
    } else {
        child_specs
            .iter()
            .filter(|(name, _, optional, _)| *optional || !peer_shadowed.contains(name))
            .cloned()
            .collect::<Vec<ChildSpec>>()
            .pipe(Arc::new)
    };
    Ok(match catalogs_for_children(ctx, pending.resolves_children_through_catalogs) {
        Some(catalogs) => child_specs
            .iter()
            .cloned()
            .collect::<Vec<ChildSpec>>()
            .pipe(|specs| resolve_catalog_child_specs(specs, catalogs))?
            .pipe(Arc::new),
        None => child_specs,
    })
}

/// What every child edge of one occurrence resolves against.
pub(super) struct ChildSeedScope<'s> {
    /// The parent's recorded snapshot, kept when it landed on its prior
    /// entry. An *updated* parent (one that landed on a different
    /// version than the lockfile recorded, or a new dep) discards its
    /// `resolvedDependencies` child refs, forcing its subtree to
    /// re-resolve. A parent that freshly resolved but landed back on its
    /// previously recorded version keeps the prior child refs alive
    /// (pnpm's non-`parentPkg.updated` arm), and each child edge
    /// re-enters the reuse gate with its recorded key — so a
    /// still-satisfied subtree is reused rather than re-resolved.
    /// Re-resolving those children would re-pick open ranges (`*`) at
    /// their newest versions and churn the lockfile.
    pub(super) prior_children_snapshot: Option<&'s SnapshotEntry>,
    /// This importer's direct-dep versions, snapshotted once for the
    /// whole child fanout instead of locking per edge.
    pub(super) direct_versions: Option<Arc<super::super::workspace_ctx::DirectDepVersions>>,
    pub(super) declaring_dir: Option<Arc<Path>>,
    pub(super) parent_is_workspace: bool,
}

impl<'s> ChildSeedScope<'s> {
    pub(super) fn of(ctx: &'s TreeCtx, pending: &PendingNode) -> Self {
        Self {
            prior_children_snapshot: pending
                .prior_key
                .as_ref()
                .filter(|key| landed_on_prior_entry(key, &pending.id))
                .and_then(|key| {
                    ctx.workspace.wanted_lockfile.as_ref()?.snapshots.as_ref()?.get(key)
                }),
            direct_versions: lock_recoverable(&ctx.workspace.direct_dep_versions)
                .get(&ctx.importer_id)
                .map(Arc::clone),
            declaring_dir: declaring_manifest_dir(ctx, &pending.result),
            parent_is_workspace: pending.result.resolved_via == "workspace",
        }
    }
}

pub(super) async fn seed_child<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    node: &FrontierNode,
    scope: &ChildSeedScope<'_>,
    spec: &ChildSpec,
) -> Result<NodeSeed, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let (wanted, prior) = child_wanted(scope, spec);
    let seed = resolve_node_seed(
        ctx,
        resolver,
        wanted,
        ChildEdge {
            ancestor_ids: &node.pending.next_ancestors,
            depth: node.pending.depth + 1,
            parent_optional: node.pending.current_is_optional,
            reuse: ReuseSource::Transitive { key: prior },
            pick_overlay: node.children_overlay.clone(),
            parent_dir: scope.declaring_dir.as_deref(),
            parent_pkg_aliases: &node.children_pkg_aliases,
            parent_is_workspace: scope.parent_is_workspace,
        },
    )
    .await?;
    warm_children_resolutions(ctx, resolver, &seed).await;
    Ok(seed)
}

/// The edge's wanted dependency and its prior key. Stale-pin refresh:
/// the edge is forced onto a higher in-range direct-dep version instead
/// of reusing the pin, so the pinned version is never resolved or
/// fetched.
pub(super) fn child_wanted(
    scope: &ChildSeedScope<'_>,
    (name, range, optional, injected): &ChildSpec,
) -> (WantedDependency, Option<PkgNameVerPeer>) {
    let mut wanted = WantedDependency {
        alias: Some(name.clone()),
        bare_specifier: Some(range.clone()),
        optional: Some(*optional),
        injected: injected.then_some(true),
        ..WantedDependency::default()
    };
    let mut prior =
        scope.prior_children_snapshot.and_then(|snapshot| prior_child_key(snapshot, name, range));
    if let Some(higher) = prior
        .as_ref()
        .and_then(|key| key.suffix.version_semver().cloned())
        .zip(range.parse::<node_semver::Range>().ok())
        .and_then(|(pinned, parsed)| {
            higher_direct_dep_version(scope.direct_versions.as_deref(), name, &pinned, &parsed)
        })
    {
        wanted.bare_specifier = Some(higher.to_string());
        prior = None;
    }
    (wanted, prior)
}

/// Whether the `parent → child` edge closes a dependency cycle's
/// *second* lap. The first re-entry of a cycle is kept (so the
/// cycle-closing dependency edge appears in the tree and the lockfile
/// snapshot); only the repeat of the full `parent … child` sequence is
/// dropped.
pub(crate) fn parent_ids_contain_sequence(
    pkg_ids: &[String],
    pkg_id1: &str,
    pkg_id2: &str,
) -> bool {
    let Some(pkg1_index) = pkg_ids.iter().position(|id| id == pkg_id1) else {
        return false;
    };
    if pkg1_index == pkg_ids.len() - 1 {
        return false;
    }
    let Some(pkg2_index) = pkg_ids.iter().rposition(|id| id == pkg_id2) else {
        return false;
    };
    pkg1_index < pkg2_index && pkg2_index != pkg_ids.len() - 1
}

/// Whether a freshly resolved node landed back on its previously
/// recorded lockfile entry — the non-`updated` arm, which keeps the
/// prior child refs alive.
pub(super) fn landed_on_prior_entry(prior_key: &PkgNameVerPeer, resolved_pkg_id: &str) -> bool {
    prior_key.without_peer().to_string() == pnpm_deps_path::remove_suffix(resolved_pkg_id)
}

/// The package names the npm picker may consult the preferred-versions
/// overlay under for one wanted edge: the alias itself, plus the real
/// package name from [`real_package_name_of`] when it differs (the
/// inner target of an `npm:` alias or the folded `@jsr/...` name of a
/// `jsr:` specifier) — mirroring the name derivation in the npm
/// resolver's `parse_bare_specifier`, which keys its overlay merge by
/// the resolved `spec.name` rather than the outer alias.
///
/// Borrowed and slot-shaped rather than a `Vec<String>`: every edge of
/// every walked package asks for these, and the overwhelming majority
/// resolve to one borrowed alias.
pub(super) fn overlay_lookup_names<'edge>(
    alias: Option<&'edge str>,
    bare_specifier: Option<&'edge str>,
) -> [Option<Cow<'edge, str>>; 2] {
    let alias = alias.filter(|alias| !alias.is_empty());
    let real_name = real_package_name_of(alias, bare_specifier)
        .filter(|real_name| alias.is_none_or(|alias| alias != real_name.as_ref()));
    [alias.map(Cow::Borrowed), real_name]
}

/// Whether this package's child specifiers pass through the importer's
/// catalogs, which makes them a property of the resolving importer
/// rather than of the package id — the one input
/// [`fn@crate::resolve_dependency_tree::workspace_ctx::recorded_children_match`] cannot compare, since the recorded
/// context does not carry the catalogs the recording importer used.
pub(super) fn resolves_children_through_catalogs(
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> bool {
    result.resolved_via == "workspace" && result.id.as_str().starts_with("file:")
}

pub(super) fn catalogs_for_children(
    ctx: &TreeCtx,
    resolves_children_through_catalogs: bool,
) -> Option<&Catalogs> {
    (resolves_children_through_catalogs && !ctx.catalogs.is_empty()).then_some(&ctx.catalogs)
}

pub(super) fn resolve_catalog_child_specs(
    child_specs: Vec<ChildSpec>,
    catalogs: &Catalogs,
) -> Result<Vec<ChildSpec>, ResolveDependencyTreeError> {
    child_specs
        .into_iter()
        .map(|(name, range, optional, injected)| {
            resolve_catalog_specifier(name, range, catalogs)
                .map(|(name, range)| (name, range, optional, injected))
        })
        .collect()
}
