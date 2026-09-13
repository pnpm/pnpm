use super::DirectDepsByImporter;
use indexmap::IndexMap;
use pnpm_lockfile::{PackageKey, ProjectSnapshot};
use std::collections::HashMap;

/// Build a [`DirectDepsByImporter`] from the lockfile's `importers:`
/// section, restricted to the supplied dependency groups.
///
/// Peer-only entries don't belong in the direct-deps map because peers
/// materialize through their host.
///
/// Accepts an iterator over `(importer_id, &ProjectSnapshot)` pairs
/// rather than the lockfile's full `&HashMap` so the caller can
/// restrict the input to the importer set actually being installed.
/// Today the frozen-lockfile call site passes the full `importers`
/// map — workspace install (pnpm/pacquet#431) landed in [#443] and
/// pacquet now installs every entry — so the iterator-shaped
/// signature lets future selected-projects (`--filter`) installs
/// pass a filtered iterator without touching this function. The
/// `link:` workspace-sibling entries are skipped via
/// [`pnpm_lockfile::ImporterDepVersion::as_regular`] inside the
/// loop.
///
/// [#443]: https://github.com/pnpm/pacquet/pull/443
pub fn build_direct_deps_by_importer<'a, Iter>(
    importers: Iter,
    dependency_groups: impl IntoIterator<Item = pnpm_package_manifest::DependencyGroup>,
) -> DirectDepsByImporter
where
    Iter: IntoIterator<Item = (&'a String, &'a ProjectSnapshot)>,
{
    let mut result: DirectDepsByImporter = IndexMap::new();
    let mut importers: Vec<_> = importers.into_iter().collect();
    importers.sort_by(|a, b| a.0.cmp(b.0));
    let dependency_groups: Vec<_> = dependency_groups.into_iter().collect();
    for (importer_id, project_snapshot) in importers {
        let resolved_by_alias = resolved_keys_by_alias(project_snapshot, &dependency_groups);
        let deps = ordered_direct_deps(project_snapshot, &dependency_groups, &resolved_by_alias);
        result.insert(importer_id.clone(), deps);
    }
    result
}

/// Package identity follows the direct-linker's caller precedence: the
/// first group that resolves an alias owns it.
fn resolved_keys_by_alias(
    project_snapshot: &ProjectSnapshot,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
) -> HashMap<String, PackageKey> {
    let mut resolved_by_alias = HashMap::new();
    for group in dependency_groups
        .iter()
        .filter(|group| !matches!(group, pnpm_package_manifest::DependencyGroup::Peer))
    {
        let Some(map) = project_snapshot.get_map_by_group(*group) else {
            continue;
        };
        for (name, spec) in map {
            let Some(key) = spec.version.resolved_key(name) else {
                continue;
            };
            resolved_by_alias
                .entry(name.to_string())
                .or_insert(key);
        }
    }
    resolved_by_alias
}

/// Key positions follow pnpm's manifest merge order.
fn ordered_direct_deps(
    project_snapshot: &ProjectSnapshot,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
    resolved_by_alias: &HashMap<String, PackageKey>,
) -> IndexMap<String, PackageKey> {
    use pnpm_package_manifest::DependencyGroup;

    let mut deps: IndexMap<String, PackageKey> = IndexMap::new();
    for group in [
        DependencyGroup::Dev,
        DependencyGroup::Prod,
        DependencyGroup::Optional,
    ]
    .into_iter()
    .filter(|group| dependency_groups.contains(group))
    {
        let Some(map) = project_snapshot.get_map_by_group(group) else {
            continue;
        };
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by_cached_key(|entry| entry.0.to_string());
        for (name, _) in entries {
            let alias = name.to_string();
            let Some(key) = resolved_by_alias.get(&alias) else {
                continue;
            };
            deps
                .entry(alias)
                .or_insert_with(|| key.clone());
        }
    }
    deps
}
