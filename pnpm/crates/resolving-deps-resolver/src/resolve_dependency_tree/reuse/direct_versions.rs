use super::{
    Arc, BTreeMap, DirectDepVersions, HashSet, PkgName, PkgNameVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry, TreeCtx, WantedSpec, lock_recoverable,
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
    wanted: &[WantedSpec],
) -> HashSet<PkgName> {
    let lockfile = ctx.workspace.reuse.lockfile.as_deref();
    let prior = lockfile.and_then(|lockfile| lockfile.importers.get(importer_id));
    let mut changed = lock_recoverable(&ctx.workspace.versions.changed_direct_deps);
    for (alias, spec, _optional, _injected) in wanted {
        let unchanged = prior
            .and_then(|importer| importer_dep_specifier(importer, alias))
            .is_some_and(|recorded| {
                recorded == spec || catalog_specifier_unchanged(lockfile, recorded, alias, spec)
            });
        if !unchanged && let Ok(name) = alias.parse::<PkgName>() {
            changed.insert(name);
        }
    }
    changed.clone()
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

/// Store the resolved (parsed) direct-dep versions for the per-edge
/// stale-pin refresh. See [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::direct_dep_versions`].
///
/// [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::direct_dep_versions`]: crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::direct_dep_versions
pub(in super::super) fn record_direct_dep_versions(
    ctx: &TreeCtx,
    level: &BTreeMap<String, Vec<String>>,
) {
    let mut versions = lock_recoverable(&ctx.workspace.versions.direct_dep_versions);
    let by_name = Arc::make_mut(&mut *versions);
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

/// True when `snapshot` depends on one of the workspace's changed direct
/// deps (see [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::changed_direct_deps`]).
///
/// [`crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::changed_direct_deps`]: crate::resolve_dependency_tree::workspace_ctx::WorkspacePreferredVersions::changed_direct_deps
pub(super) fn reused_parent_has_changed_direct_child(
    ctx: &TreeCtx,
    snapshot: &SnapshotEntry,
) -> bool {
    // Copy the (small) changed set out and drop the lock before scanning.
    let changed_set = {
        let changed = lock_recoverable(&ctx.workspace.versions.changed_direct_deps);
        if changed.is_empty() {
            return false;
        }
        changed.clone()
    };
    let depends_on = |map: Option<&std::collections::HashMap<PkgName, SnapshotDepRef>>| {
        map.is_some_and(|deps| {
            deps.keys()
                .any(|name| changed_set.contains(name))
        })
    };
    depends_on(snapshot.dependencies.as_ref())
        || depends_on(snapshot.optional_dependencies.as_ref())
}

/// Reuse-decline gate: whether `prior_key`'s prior snapshot depends on a
/// changed direct dep.
pub(in super::super) fn node_depends_on_changed_direct_dep(
    ctx: &TreeCtx,
    prior_key: Option<&PkgNameVerPeer>,
) -> bool {
    prior_key
        .and_then(|key| {
            ctx.workspace.reuse.lockfile
                .as_ref()?
                .snapshots
                .as_ref()?
                .get(key)
        })
        .is_some_and(|snapshot| reused_parent_has_changed_direct_child(ctx, snapshot))
}

/// The highest resolved direct-dependency version of `name` strictly
/// above `pinned` that still satisfies `range`, or `None`. Anchored to
/// direct deps (the deterministic, resolved-before-the-walk signal).
/// `direct_versions` is the workspace's snapshot, taken once per walked
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
/// lockfile) and record their resolved versions into `direct_dep_versions`
/// before any importer walks its transitive subtrees.
pub(crate) async fn preseed_changed_direct_versions<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: &[WantedSpec],
    importer_id: &str,
    parent_pkg_aliases: &Arc<super::ParentPkgAliases>,
) -> Result<(), super::ResolveDependencyTreeError>
where
    Chain: super::Resolver + ?Sized,
{
    let changed = lock_recoverable(&ctx.workspace.versions.changed_direct_deps).clone();
    if changed.is_empty() {
        return Ok(());
    }
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for spec in wanted
        .iter()
        .filter(|(alias, ..)| {
            alias
                .parse::<PkgName>()
                .is_ok_and(|name| changed.contains(&name))
        })
    {
        if let Some((name, version)) =
            preseed_one_changed_direct(ctx, resolver, spec.clone(), importer_id, parent_pkg_aliases)
                .await?
        {
            versions
                .entry(name)
                .or_default()
                .push(version);
        }
    }
    if !versions.is_empty() {
        record_direct_dep_versions(ctx, &versions);
    }
    Ok(())
}

async fn preseed_one_changed_direct<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    spec: WantedSpec,
    importer_id: &str,
    parent_pkg_aliases: &Arc<super::ParentPkgAliases>,
) -> Result<Option<(String, String)>, super::ResolveDependencyTreeError>
where
    Chain: super::Resolver + ?Sized,
{
    let mut wanted_dep = super::super::wanted_from_spec(spec);
    let reuse = super::ReuseSource::Importer { importer_id: importer_id.to_string() };
    let prior_key = reuse.prior_key(ctx, &wanted_dep);
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
    let resolved =
        super::super::walk::resolve_edge(ctx, resolver, &mut wanted_dep, &edge, prior_key.as_ref())
            .await?;
    Ok(resolved.and_then(|result| {
        let name_ver = result.package.name_ver.as_ref()?;
        Some((name_ver.name.to_string(), name_ver.suffix.to_string()))
    }))
}
