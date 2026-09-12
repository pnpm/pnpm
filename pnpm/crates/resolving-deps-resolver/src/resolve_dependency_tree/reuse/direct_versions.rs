use super::{
    Arc, BTreeMap, DirectDepVersions, HashSet, PkgName, PkgNameVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry, TreeCtx, WantedSpec, lock_recoverable,
};

/// Record the importer direct deps whose manifest specifier differs from
/// the prior lockfile's recorded specifier (a new dep counts as changed).
/// See [`WorkspaceTreeCtx::changed_direct_deps`].
///
/// Called for the importer's *manifest* wave only, never for
/// auto-installed peers: hoisted peers have no importer-snapshot entry
/// to compare against, so recording them would mark them "changed" on
/// every install and permanently decline subtree reuse for anything
/// depending on them.
///
/// [`WorkspaceTreeCtx::changed_direct_deps`]: super::super::WorkspaceTreeCtx::changed_direct_deps
pub(crate) fn record_changed_direct_deps(
    ctx: &TreeCtx,
    importer_id: &str,
    wanted: &[WantedSpec],
) -> HashSet<PkgName> {
    let lockfile = ctx.workspace.wanted_lockfile.as_deref();
    let prior = lockfile.and_then(|lockfile| lockfile.importers.get(importer_id));
    let mut changed = lock_recoverable(&ctx.workspace.changed_direct_deps);
    let bucket = changed.entry(importer_id.to_string()).or_default();
    for (alias, spec, _optional, _injected) in wanted {
        let unchanged = prior
            .and_then(|importer| importer_dep_specifier(importer, alias))
            .is_some_and(|recorded| {
                recorded == spec || catalog_specifier_unchanged(lockfile, recorded, alias, spec)
            });
        if !unchanged && let Ok(name) = alias.parse::<PkgName>() {
            bucket.insert(name);
        }
    }
    bucket.clone()
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
/// per-edge stale-pin refresh. See [`WorkspaceTreeCtx::direct_dep_versions`].
///
/// [`WorkspaceTreeCtx::direct_dep_versions`]: super::super::WorkspaceTreeCtx::direct_dep_versions
pub(in super::super) fn record_direct_dep_versions(
    ctx: &TreeCtx,
    importer_id: &str,
    level: &BTreeMap<String, Vec<String>>,
) {
    let mut versions = lock_recoverable(&ctx.workspace.direct_dep_versions);
    let by_name = Arc::make_mut(versions.entry(importer_id.to_string()).or_default());
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
/// deps (see [`WorkspaceTreeCtx::changed_direct_deps`]).
///
/// [`WorkspaceTreeCtx::changed_direct_deps`]: super::super::WorkspaceTreeCtx::changed_direct_deps
pub(super) fn reused_parent_has_changed_direct_child(
    ctx: &TreeCtx,
    snapshot: &SnapshotEntry,
) -> bool {
    // Copy the (small) changed set out and drop the lock before scanning.
    let importer_changed = {
        let changed = lock_recoverable(&ctx.workspace.changed_direct_deps);
        match changed.get(&ctx.importer_id) {
            Some(set) if !set.is_empty() => set.clone(),
            _ => return false,
        }
    };
    let depends_on = |map: Option<&std::collections::HashMap<PkgName, SnapshotDepRef>>| {
        map.is_some_and(|deps| deps.keys().any(|name| importer_changed.contains(name)))
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
        .and_then(|key| ctx.workspace.wanted_lockfile.as_ref()?.snapshots.as_ref()?.get(key))
        .is_some_and(|snapshot| reused_parent_has_changed_direct_child(ctx, snapshot))
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
    let resolved = direct_versions?.get(name)?;
    resolved.iter().filter(|&version| version > pinned && range.satisfies(version)).max().cloned()
}
