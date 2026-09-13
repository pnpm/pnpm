use super::{
    Catalogs, DependencyGroup, HashMap, PackageManifest, ResolveDependencyTreeError, WantedSpec,
    importer_injected_dependency_names, importer_optional_dependency_names,
    resolve_catalog_specifiers,
};

/// Build the importer's direct-dependency wanted specs: the manifest's
/// `dependencies` (plus, when `auto_install_peers`, its own
/// `peerDependencies`) tagged with the right `optional` / `injected`
/// flags and with `catalog:` specifiers resolved.
///
/// An alias declared in several groups yields one spec, merged by
/// spreading the groups in order: `peerDependencies` first (when
/// `auto_install_peers`), then `devDependencies` < `dependencies` <
/// `optionalDependencies`, a later group's range replacing an earlier
/// one — matching `filterDependenciesByType` in
/// `@pnpm/pkg-manifest.utils` (`{...dev, ...prod, ...optional}`), so a
/// regular dep wins over a devDependency of the same alias, and either
/// wins over its peer range.
///
/// Shared by [`fn@crate::resolve_importer`] (which walks them) and the
/// `time-based` cutoff pre-pass in [`fn@crate::resolve_workspace`]
/// (which only needs the resolved direct-dep publish dates), so both
/// see the identical direct-dep set — the importer-dep computation runs
/// once before resolving an importer's deps.
pub(crate) fn importer_direct_wanted_specs<DependencyGroupList>(
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    auto_install_peers: bool,
    catalogs: &Catalogs,
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let groups = importer_dependency_groups(dependency_groups, auto_install_peers);
    let optional_names = importer_optional_dependency_names(manifest);
    let injected_names = importer_injected_dependency_names(manifest);
    let mut order: Vec<&str> = Vec::new();
    let mut ranges: HashMap<&str, &str> = HashMap::default();
    for (name, range) in manifest.dependencies(groups) {
        if !crate::is_valid_dependency_alias(name) {
            return Err(ResolveDependencyTreeError::InvalidDependencyName {
                parent: "The current package".to_string(),
                alias: name.to_string(),
            });
        }
        if ranges.insert(name, range).is_none() {
            order.push(name);
        }
    }
    let wanted: Vec<WantedSpec> = order
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                ranges[name].to_string(),
                optional_names.contains(name),
                injected_names.contains(name),
            )
        })
        .collect();
    resolve_catalog_specifiers(wanted, catalogs)
}

fn importer_dependency_groups(
    dependency_groups: impl IntoIterator<Item = DependencyGroup>,
    auto_install_peers: bool,
) -> Vec<DependencyGroup> {
    let included: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
    let mut groups: Vec<DependencyGroup> = Vec::new();
    if auto_install_peers || included.contains(&DependencyGroup::Peer) {
        groups.push(DependencyGroup::Peer);
    }
    groups.extend(
        [
            DependencyGroup::Dev,
            DependencyGroup::Prod,
            DependencyGroup::Optional,
        ]
        .into_iter()
        .filter(|group| included.contains(group)),
    );
    groups
}
