use node_semver::Range;
use pacquet_lockfile::{Lockfile, PkgName, ProjectSnapshot, ResolvedDependencySpec};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::{HashMap, HashSet};

pub(crate) fn try_fast_update_importers(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
) -> Option<Lockfile> {
    let mut candidate = lockfile.clone();
    let mut changed = false;
    let mut dropped = HashSet::new();
    for (importer_id, manifest) in manifests {
        let importer = candidate.importers.get_mut(importer_id)?;
        let manifest_specifiers = manifest
            .dependencies([DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional])
            .collect::<HashMap<_, _>>();
        for (alias, specifier) in &manifest_specifiers {
            let alias = PkgName::parse(*alias).ok()?;
            let dependency = importer_dependency_mut(importer, &alias)?;
            if dependency.specifier == *specifier {
                continue;
            }
            let range = Range::parse(specifier).ok()?;
            let version = dependency.version.ver_peer()?.version_semver()?;
            if !version.satisfies(&range) {
                return None;
            }
            dependency.specifier = (*specifier).to_string();
            changed = true;
        }
        let removed = remove_dependencies_absent_from(importer, &manifest_specifiers);
        changed |= !removed.is_empty();
        dropped.extend(removed);
    }
    if !dropped.is_empty() {
        if !peer_suffixes_are_independent_of(&candidate, &dropped) {
            return None;
        }
        crate::fast_update_lockfile::prune_unreachable_packages(&mut candidate);
    }
    changed.then_some(candidate)
}

/// Drop every dependency the importer records that the manifest no longer
/// declares, returning their names.
fn remove_dependencies_absent_from(
    importer: &mut ProjectSnapshot,
    manifest_specifiers: &HashMap<&str, &str>,
) -> HashSet<PkgName> {
    let declared = |alias: &PkgName| manifest_specifiers.contains_key(alias.to_string().as_str());
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
