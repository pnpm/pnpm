use super::{
    Arc, BTreeMap, DependencySpec, DirectDepVersions, PkgName, PkgNameVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry, TreeCtx, lock_recoverable,
};

/// Record the importer direct deps whose manifest specifier differs from
/// the prior lockfile's recorded specifier (a new dep counts as changed).
/// See [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::changed_direct_deps`].
///
/// Called for the importer's *manifest* wave only, never for
/// auto-installed peers: hoisted peers have no importer-snapshot entry
/// to compare against, so recording them would mark them "changed" on
/// every install and permanently decline subtree reuse for anything
/// depending on them.
///
/// [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::changed_direct_deps`]: crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::changed_direct_deps
pub(crate) fn record_changed_direct_deps(
    ctx: &TreeCtx,
    importer_id: &str,
    wanted: &[DependencySpec],
) {
    let Some(lockfile) = ctx.workspace.reuse.lockfile.as_deref() else {
        return;
    };
    let prior = lockfile.importers.get(importer_id);
    let mut changed = lock_recoverable(&ctx.workspace.versions.changed_direct_deps);
    let set = Arc::make_mut(changed.entry(importer_id.to_string()).or_default());
    for DependencySpec { alias, bare_specifier: spec, .. } in wanted {
        if !direct_dep_specifier_unchanged(lockfile, prior, alias, spec)
            && let Ok(name) = alias.parse::<PkgName>()
        {
            set.insert(name);
        }
    }
}

fn direct_dep_specifier_unchanged(
    lockfile: &pnpm_lockfile::Lockfile,
    prior: Option<&ProjectSnapshot>,
    alias: &str,
    spec: &str,
) -> bool {
    prior
        .and_then(|importer| importer_dep_specifier(importer, alias))
        .is_some_and(|recorded| {
            recorded == spec || catalog_specifier_unchanged(Some(lockfile), recorded, alias, spec)
        })
}

/// Whether a `catalog:`-recorded direct dep still resolves to the same
/// underlying range. The wanted specs reaching
/// [`fn@record_changed_direct_deps`] have their `catalog:` protocol
/// already replaced by the catalog's range
/// ([`fn@resolve_catalog_specifiers`]), while the importer snapshot
/// records the literal `catalog:` / `catalog:<name>` form — comparing
/// those directly would mark every catalog-managed dep as changed on
/// every install, and the changed-direct-dep reuse gate would then
/// re-resolve (and drift) every subtree depending on one. The edge is
/// unchanged when the lockfile's `catalogs:` snapshot recorded the same
/// range for this alias.
///
/// [`fn@resolve_catalog_specifiers`]: super::super::resolve_catalog_specifiers
pub(super) fn catalog_specifier_unchanged(
    lockfile: Option<&pnpm_lockfile::Lockfile>,
    recorded: &str,
    alias: &str,
    resolved_spec: &str,
) -> bool {
    let Some(catalog_name) = recorded.strip_prefix("catalog:") else {
        return false;
    };
    let catalog_name = if catalog_name.is_empty() { "default" } else { catalog_name };
    lockfile
        .and_then(|lockfile| lockfile.catalogs.as_ref())
        .and_then(|catalogs| catalogs.get(catalog_name))
        .and_then(|catalog| catalog.get(alias))
        .is_some_and(|entry| entry.specifier == resolved_spec)
}

/// The recorded specifier for direct-dep `alias` across the importer's
/// prod / dev / optional dependency maps in the prior lockfile.
pub(super) fn importer_dep_specifier<'a>(
    importer: &'a ProjectSnapshot,
    alias: &str,
) -> Option<&'a str> {
    let name: PkgName = alias.parse().ok()?;
    let lookup = |map: Option<&'a ResolvedDependencyMap>| map.and_then(|deps| deps.get(&name));
    lookup(importer.dependencies.as_ref())
        .or_else(|| lookup(importer.optional_dependencies.as_ref()))
        .or_else(|| lookup(importer.dev_dependencies.as_ref()))
        .map(|dep| dep.specifier.as_str())
}

/// Store the importer's resolved (parsed) direct-dep versions for the
/// per-edge stale-pin refresh, together with any workspace-wide changed
/// direct-dep versions for names this importer does not declare directly.
/// See [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::direct_dep_versions`].
///
/// [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::direct_dep_versions`]: crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::direct_dep_versions
pub(in super::super) fn record_direct_dep_versions(
    ctx: &TreeCtx,
    importer_id: &str,
    level: &BTreeMap<String, Vec<String>>,
) {
    let changed_versions =
        Arc::clone(&lock_recoverable(&ctx.workspace.versions.changed_direct_dep_versions));
    let mut versions = lock_recoverable(&ctx.workspace.versions.direct_dep_versions);
    let by_name = Arc::make_mut(versions.entry(importer_id.to_string()).or_default());
    extend_parsed_versions(by_name, level);
    for (name, candidates) in changed_versions.iter() {
        by_name.entry(name.clone()).or_insert_with(|| candidates.clone());
    }
}

fn extend_parsed_versions(by_name: &mut DirectDepVersions, level: &BTreeMap<String, Vec<String>>) {
    for (name, level_versions) in level {
        let bucket = by_name.entry(name.clone()).or_default();
        for version in level_versions {
            let Ok(parsed) = version.parse::<node_semver::Version>() else { continue };
            if !bucket.contains(&parsed) {
                bucket.push(parsed);
            }
        }
    }
}

/// True when `snapshot` depends on one of this importer's changed direct
/// deps, or on a dependency whose pinned version differs from a version
/// resolved for a changed direct dep in another importer.
fn reused_parent_has_changed_direct_child(
    ctx: &TreeCtx,
    snapshot: &SnapshotEntry,
    scope: ChangedDirectScope,
) -> bool {
    let own_changed = lock_recoverable(&ctx.workspace.versions.changed_direct_deps)
        .get(&ctx.importer.id)
        .cloned();
    let cross_changed = match scope {
        ChangedDirectScope::EveryImporter => {
            Some(Arc::clone(&lock_recoverable(&ctx.workspace.versions.changed_direct_dep_versions)))
        }
        ChangedDirectScope::OwnImporter => None,
    };
    if own_changed.as_ref().is_none_or(|set| set.is_empty())
        && cross_changed.as_ref().is_none_or(|map| map.is_empty())
    {
        return false;
    }
    snapshot.dependencies
        .iter()
        .chain(snapshot.optional_dependencies.iter())
        .flatten()
        .any(|(name, dep_ref)| {
            own_changed
                .as_ref()
                .is_some_and(|set| set.contains(name))
                || cross_changed
                    .as_deref()
                    .is_some_and(|cross| cross_importer_changed_version(cross, name, dep_ref))
        })
}

fn cross_importer_changed_version(
    cross_changed: &DirectDepVersions,
    name: &PkgName,
    dep_ref: &SnapshotDepRef,
) -> bool {
    let (target_name, ver_peer) = match dep_ref {
        SnapshotDepRef::Plain(ver_peer) => (name, ver_peer),
        SnapshotDepRef::Alias(key) => (&key.name, &key.suffix),
        SnapshotDepRef::Link(_) => return false,
    };
    let candidates = match &target_name.scope {
        None => cross_changed.get(target_name.bare.as_str()),
        Some(_) => cross_changed.get(&target_name.to_string()),
    };
    candidates
        .zip(ver_peer.version_semver())
        .is_some_and(|(versions, pinned)| {
            versions
                .iter()
                .any(|version| version != pinned)
        })
}

/// Which importers' changed direct deps the reuse-decline gate counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum ChangedDirectScope {
    /// Only the importer the snapshot belongs to.
    OwnImporter,
    /// Every importer, so a dep another importer changed declines reuse too.
    EveryImporter,
}

/// Reuse-decline gate: whether `prior_key`'s prior snapshot depends on a
/// changed direct dep.
pub(in super::super) fn node_depends_on_changed_direct_dep(
    ctx: &TreeCtx,
    prior_key: Option<&PkgNameVerPeer>,
) -> bool {
    node_depends_on_changed_direct_dep_with(ctx, prior_key, ChangedDirectScope::EveryImporter)
}

fn node_depends_on_changed_direct_dep_with(
    ctx: &TreeCtx,
    prior_key: Option<&PkgNameVerPeer>,
    scope: ChangedDirectScope,
) -> bool {
    let Some(key) = prior_key else {
        return false;
    };
    ctx.workspace.reuse.lockfile
        .as_ref()
        .and_then(|lf| lf.snapshots.as_ref()?.get(key))
        .is_some_and(|snapshot| reused_parent_has_changed_direct_child(ctx, snapshot, scope))
}

/// The highest resolved direct-dependency version of `name` strictly
/// above `pinned` that still satisfies `range`, or `None`. Anchored to
/// direct deps (the deterministic, resolved-before-the-walk signal).
/// `direct_versions` is the importer's snapshot, taken once per walked
/// occurrence as it seeds its children.
pub(in super::super) fn higher_direct_dep_version(
    direct_versions: Option<&DirectDepVersions>,
    name: &str,
    pinned: &node_semver::Version,
    range: &node_semver::Range,
) -> Option<node_semver::Version> {
    direct_versions?
        .get(name)?
        .iter()
        .filter(|&version| version > pinned && range.satisfies(version))
        .max()
        .cloned()
}

/// Resolve any changed direct dependencies up front (when reusing a prior
/// lockfile) and record their resolved versions into
/// `changed_direct_dep_versions` before any importer walks its transitive
/// subtrees.
pub(crate) async fn preseed_changed_direct_versions<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: &[DependencySpec],
    importer_id: &str,
    parent_pkg_aliases: &Arc<super::ParentPkgAliases>,
) -> Result<(), super::ResolveDependencyTreeError>
where
    Chain: super::Resolver + ?Sized,
{
    let Some(lockfile) = ctx.workspace.reuse.lockfile.as_deref() else {
        return Ok(());
    };
    let prior = lockfile.importers.get(importer_id);
    let resolved = futures_util::future::try_join_all(
        wanted
            .iter()
            .filter(|spec| {
                !direct_dep_specifier_unchanged(lockfile, prior, &spec.alias, &spec.bare_specifier)
            })
            .map(|spec| {
                preseed_one_changed_direct(
                    ctx,
                    resolver,
                    spec.clone(),
                    importer_id,
                    parent_pkg_aliases,
                )
            }),
    )
    .await?;
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, version) in resolved.into_iter().flatten() {
        versions
            .entry(name)
            .or_default()
            .push(version);
    }
    if !versions.is_empty() {
        let mut changed_versions =
            lock_recoverable(&ctx.workspace.versions.changed_direct_dep_versions);
        extend_parsed_versions(Arc::make_mut(&mut *changed_versions), &versions);
    }
    Ok(())
}

async fn preseed_one_changed_direct<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    spec: DependencySpec,
    importer_id: &str,
    parent_pkg_aliases: &Arc<super::ParentPkgAliases>,
) -> Result<Option<(String, String)>, super::ResolveDependencyTreeError>
where
    Chain: super::Resolver + ?Sized,
{
    let mut wanted_dep = super::super::wanted_from_spec(spec);
    let reuse = super::ReuseSource::Importer { importer_id: importer_id.to_string() };
    let prior_key = reuse.prior_key(ctx, &wanted_dep);
    let scope = ChangedDirectScope::OwnImporter;
    if ctx.workspace.reuse.subtrees
        && reuse.allows_reuse()
        && !node_depends_on_changed_direct_dep_with(ctx, prior_key.as_ref(), scope)
        && let Some(reused) = super::try_reuse_node(ctx, &wanted_dep, prior_key.as_ref(), 0)
        && super::reuse_matches_peer_scope(ctx, &reused, parent_pkg_aliases)
    {
        return Ok(reused.result.package.name_ver
            .as_ref()
            .map(|name_ver| (name_ver.name.to_string(), name_ver.suffix.to_string())));
    }
    let ancestors = Arc::new(Vec::new());
    let edge = super::ChildEdge {
        ancestor_ids: &ancestors,
        depth: 0,
        parent_optional: false,
        reuse,
        pick_overlay: ctx.options.base.version.preferred_versions_overlay.clone(),
        parent_dir: None,
        parent_pkg_aliases,
        parent_is_workspace: false,
    };
    let Some(resolved) = super::super::walk::try_resolve_edge_cached(
        ctx,
        resolver,
        &mut wanted_dep,
        &edge,
        prior_key.as_ref(),
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(resolved.package.name_ver
        .as_ref()
        .map(|name_ver| (name_ver.name.to_string(), name_ver.suffix.to_string())))
}
