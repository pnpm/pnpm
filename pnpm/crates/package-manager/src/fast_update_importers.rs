use crate::fast_update_compose::Drift;
use crate::fast_update_lockfile::GraphEdits;
use node_semver::Range;
use pacquet_lockfile::{
    Lockfile, PkgName, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::{HashMap, HashSet};

/// Each manifest alias with its specifier and the group it is
/// effectively declared under. Keyed by [`PkgName`] so membership tests
/// against importer records need no per-dependency conversions.
type ManifestDependencies<'manifest> = HashMap<PkgName, (&'manifest str, DependencyGroup)>;

/// The prepared inputs [`apply_importers_update`] replays: each
/// importer's manifest map, built once so detection and application share
/// the parsed aliases.
pub(crate) struct ImportersPlan<'a, 'manifest> {
    manifest_dependencies: Vec<(&'a String, ManifestDependencies<'manifest>)>,
}

/// Whether the importers' records diverge from the manifests, without
/// cloning anything. [`Drift::Resolve`] when an alias cannot be parsed,
/// which the resolver reports.
pub(crate) fn detect_importers_drift<'a, 'manifest>(
    lockfile: &Lockfile,
    manifests: &'a [(String, &'manifest PackageManifest)],
) -> Drift<ImportersPlan<'a, 'manifest>> {
    let mut manifest_dependencies: Vec<(&String, ManifestDependencies<'_>)> = Vec::new();
    for (importer_id, manifest) in manifests {
        // Later groups overwrite, so each alias ends at the group
        // `satisfies_package_manifest` expects it recorded under when it
        // appears in several: optional wins over prod, prod over dev.
        let mut dependencies = HashMap::new();
        for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
            for (name, specifier) in manifest.dependencies([group]) {
                let Ok(name) = PkgName::parse(name) else {
                    return Drift::Resolve;
                };
                dependencies.insert(name, (specifier, group));
            }
        }
        manifest_dependencies.push((importer_id, dependencies));
    }
    if manifest_dependencies
        .iter()
        .any(|(importer_id, dependencies)| importer_diverges(lockfile, importer_id, dependencies))
    {
        Drift::Absorb(ImportersPlan { manifest_dependencies })
    } else {
        Drift::Clean
    }
}

/// Replay the manifests' drift onto `candidate`: compatible specifier
/// changes, group moves, and removals, with the dropped aliases and
/// optionality moves recorded in `edits` for the shared epilogue.
/// `false` — an incompatible or non-semver change, or an alias the
/// lockfile does not record — leaves the caller on the full-resolution
/// path.
pub(crate) fn apply_importers_update(
    candidate: &mut Lockfile,
    plan: &ImportersPlan<'_, '_>,
    edits: &mut GraphEdits,
) -> bool {
    for (importer_id, manifest_dependencies) in &plan.manifest_dependencies {
        let Some(importer) = candidate.importers.get_mut(importer_id.as_str()) else {
            return false;
        };
        for (alias, (specifier, target)) in manifest_dependencies {
            let Some(dependency) = importer_dependency_mut(importer, alias) else {
                return false;
            };
            if dependency.specifier != *specifier {
                let Ok(range) = Range::parse(specifier) else {
                    return false;
                };
                let Some(version) =
                    dependency.version.ver_peer().and_then(|ver_peer| ver_peer.version_semver())
                else {
                    return false;
                };
                if !version.satisfies(&range) {
                    return false;
                }
                dependency.specifier = (*specifier).to_string();
            }
            if let Some(source) = move_dependency(importer, alias, *target) {
                edits.moved_across_optional |=
                    source == DependencyGroup::Optional || *target == DependencyGroup::Optional;
            }
        }
        edits.dropped.extend(remove_dependencies_absent_from(importer, manifest_dependencies));
    }
    true
}

/// Whether the importer's record differs from the manifest in a way the
/// update loop would act on: a changed specifier, a dependency recorded
/// under another group, a dependency the manifest no longer declares, or
/// a manifest entry the importer does not record (which the loop turns
/// into a fallback).
fn importer_diverges(
    lockfile: &Lockfile,
    importer_id: &str,
    manifest_dependencies: &ManifestDependencies<'_>,
) -> bool {
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return false;
    };
    let recorded_but_undeclared = [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    .flat_map(HashMap::keys)
    .any(|alias| !manifest_dependencies.contains_key(alias));
    recorded_but_undeclared
        || manifest_dependencies.iter().any(|(alias, (specifier, target))| {
            let Some((recorded_in, dependency)) = importer_dependency(importer, alias) else {
                return true;
            };
            dependency.specifier != *specifier || recorded_in != *target
        })
}

fn importer_dependency<'a>(
    importer: &'a ProjectSnapshot,
    alias: &PkgName,
) -> Option<(DependencyGroup, &'a ResolvedDependencySpec)> {
    [
        (DependencyGroup::Optional, importer.optional_dependencies.as_ref()),
        (DependencyGroup::Prod, importer.dependencies.as_ref()),
        (DependencyGroup::Dev, importer.dev_dependencies.as_ref()),
    ]
    .into_iter()
    .find_map(|(group, dependencies)| {
        dependencies.and_then(|dependencies| dependencies.get(alias)).map(|spec| (group, spec))
    })
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
    manifest_dependencies: &ManifestDependencies<'_>,
) -> HashSet<PkgName> {
    let declared = |alias: &PkgName| manifest_dependencies.contains_key(alias);
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
