use node_semver::Range;
use pacquet_lockfile::{
    Lockfile, PkgName, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::{HashMap, HashSet};

pub(crate) fn try_fast_update_importers(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
) -> Option<Lockfile> {
    let mut candidate = lockfile.clone();
    let mut changed = false;
    let mut moved_across_optional = false;
    let mut dropped = HashSet::new();
    for (importer_id, manifest) in manifests {
        let importer = candidate.importers.get_mut(importer_id)?;
        // Later groups overwrite, so each alias ends at the group
        // `satisfies_package_manifest` expects it recorded under when it
        // appears in several: optional wins over prod, prod over dev.
        let mut manifest_dependencies = HashMap::new();
        for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
            for (name, specifier) in manifest.dependencies([group]) {
                manifest_dependencies.insert(name, (specifier, group));
            }
        }
        for (alias, (specifier, target)) in &manifest_dependencies {
            let alias = PkgName::parse(*alias).ok()?;
            let dependency = importer_dependency_mut(importer, &alias)?;
            if dependency.specifier != *specifier {
                let range = Range::parse(specifier).ok()?;
                let version = dependency.version.ver_peer()?.version_semver()?;
                if !version.satisfies(&range) {
                    return None;
                }
                dependency.specifier = (*specifier).to_string();
                changed = true;
            }
            if let Some(source) = move_dependency(importer, &alias, *target) {
                moved_across_optional |=
                    source == DependencyGroup::Optional || *target == DependencyGroup::Optional;
                changed = true;
            }
        }
        let removed = remove_dependencies_absent_from(importer, &manifest_dependencies);
        changed |= !removed.is_empty();
        dropped.extend(removed);
    }
    if !dropped.is_empty() {
        // Prune first: a peer-dependent snapshot that the removal itself
        // makes unreachable needs no rekeying, so only the survivors are
        // checked.
        crate::fast_update_lockfile::prune_unreachable_packages(&mut candidate);
        if !peer_suffixes_are_independent_of(&candidate, &dropped) {
            return None;
        }
        crate::fast_update_lockfile::prune_unreferenced_catalog_entries(&mut candidate);
    }
    if !dropped.is_empty() || moved_across_optional {
        crate::fast_update_lockfile::recompute_optional_flags(&mut candidate);
    }
    changed.then_some(candidate)
}

/// Move the importer's record of `alias` into `target`, returning the group
/// it was recorded under, or `None` when it is not recorded or already
/// there.
fn move_dependency(
    importer: &mut ProjectSnapshot,
    alias: &PkgName,
    target: DependencyGroup,
) -> Option<DependencyGroup> {
    let source = [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev]
        .into_iter()
        .find(|group| {
            importer_group(importer, *group)
                .as_ref()
                .is_some_and(|dependencies| dependencies.contains_key(alias))
        })?;
    if source == target {
        return None;
    }
    let source_group = importer_group(importer, source);
    let dependency = source_group.as_mut()?.remove(alias)?;
    if source_group.as_ref().is_some_and(HashMap::is_empty) {
        *source_group = None;
    }
    importer_group(importer, target).get_or_insert_default().insert(alias.clone(), dependency);
    Some(source)
}

fn importer_group(
    importer: &mut ProjectSnapshot,
    group: DependencyGroup,
) -> &mut Option<ResolvedDependencyMap> {
    match group {
        DependencyGroup::Prod => &mut importer.dependencies,
        DependencyGroup::Dev => &mut importer.dev_dependencies,
        DependencyGroup::Optional => &mut importer.optional_dependencies,
        DependencyGroup::Peer => unreachable!("peerDependencies is not an importer group"),
    }
}

/// Drop every dependency the importer records that the manifest no longer
/// declares, returning their names.
fn remove_dependencies_absent_from(
    importer: &mut ProjectSnapshot,
    manifest_dependencies: &HashMap<&str, (&str, DependencyGroup)>,
) -> HashSet<PkgName> {
    let declared = |alias: &PkgName| manifest_dependencies.contains_key(alias.to_string().as_str());
    let mut removed = HashSet::new();
    for group in [
        importer.dependencies.as_mut(),
        importer.dev_dependencies.as_mut(),
        importer.optional_dependencies.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        group.retain(|alias, _| {
            if declared(alias) {
                return true;
            }
            removed.insert(alias.clone());
            false
        });
    }
    for group in [
        &mut importer.dependencies,
        &mut importer.dev_dependencies,
        &mut importer.optional_dependencies,
    ] {
        if group.as_ref().is_some_and(HashMap::is_empty) {
            *group = None;
        }
    }
    if let Some(specifiers) = importer.specifiers.as_mut() {
        specifiers.retain(|alias, _| !removed.iter().any(|name| name.to_string() == *alias));
    }
    removed
}

/// Whether no surviving snapshot resolves a peer through one of `dropped`.
///
/// A dropped package that some snapshot reaches as a peer is embedded in
/// that snapshot's key, so removing it would rekey the dependent rather
/// than only prune. A peer suffix pnpm shortened into a hash cannot be
/// read to rule that out.
fn peer_suffixes_are_independent_of(lockfile: &Lockfile, dropped: &HashSet<PkgName>) -> bool {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return true;
    };
    snapshots.keys().all(|key| {
        let peers = key.suffix.peer();
        peers.is_empty()
            || (peers
                .trim_start_matches('(')
                .trim_end_matches(')')
                .split(")(")
                .all(|segment| segment.contains('@'))
                && !dropped.iter().any(|name| peers.contains(&format!("{name}@"))))
    })
}

fn importer_dependency_mut<'a>(
    importer: &'a mut ProjectSnapshot,
    alias: &PkgName,
) -> Option<&'a mut ResolvedDependencySpec> {
    [
        importer.optional_dependencies.as_mut(),
        importer.dependencies.as_mut(),
        importer.dev_dependencies.as_mut(),
    ]
    .into_iter()
    .find_map(|group| group.and_then(|dependencies| dependencies.get_mut(alias)))
}

#[cfg(test)]
mod tests;
