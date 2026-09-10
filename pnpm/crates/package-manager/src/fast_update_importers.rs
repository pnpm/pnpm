pub(crate) use locked_versions::locked_version_resolution_would_pick;

mod importer_update;
use importer_update::{
    ImporterUpdate, LockedInputs, apply_one_importer_update, drop_stale_importers,
};

mod locked_versions;

use crate::{
    fast_update_compose::Drift, fast_update_lockfile::GraphEdits,
    fast_update_settings::workspace_package_names,
};
use node_semver::Range;
use pnpm_lockfile::{
    Lockfile, PkgName, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// Each manifest alias with its specifier and the group it is
/// effectively declared under. Keyed by [`PkgName`] so membership tests
/// against importer records need no per-dependency conversions.
type ManifestDependencies<'manifest> = FxHashMap<PkgName, (&'manifest str, DependencyGroup)>;

/// The prepared inputs [`apply_importers_update`] replays: each
/// importer's manifest map, built once so detection and application share
/// the parsed aliases.
pub(crate) struct ImportersPlan<'a, 'manifest> {
    manifest_dependencies:
        Vec<(&'a String, &'manifest PackageManifest, ManifestDependencies<'manifest>)>,
    /// Importers no project claims, to drop before the rest replays.
    stale: Vec<String>,
    workspace_package_names: HashSet<String>,
    /// Whether `resolutionMode` resolves a direct dependency to its
    /// lowest satisfying version rather than its highest.
    resolution_picks_lowest: bool,
}

/// Whether the importers' records diverge from the manifests, without
/// cloning anything. [`Drift::Resolve`] when an alias cannot be parsed
/// (which the resolver reports) or when a changed specifier is one
/// [`apply_importers_update`] is certain to refuse — bailing here keeps
/// the compose from cloning a workspace-scale lockfile it would then
/// throw away.
pub(crate) fn detect_importers_drift<'a, 'manifest>(
    lockfile: &Lockfile,
    manifests: &'a [(String, &'manifest PackageManifest)],
    project_manifests: &[(PathBuf, &PackageManifest)],
    prune_stale_importers: bool,
    resolution_picks_lowest: bool,
) -> Drift<ImportersPlan<'a, 'manifest>> {
    // Each importer's map builds from its own manifest alone; an
    // unparsable alias anywhere still sends the compose to the resolver.
    let manifest_dependencies: Option<Vec<(&String, &PackageManifest, ManifestDependencies<'_>)>> =
        manifests
            .par_iter()
            .map(|(importer_id, manifest)| {
                let dependencies = manifest_dependency_map(manifest)?;
                Some((importer_id, *manifest, dependencies))
            })
            .collect();
    let Some(manifest_dependencies) = manifest_dependencies else {
        return Drift::Resolve;
    };
    let stale = stale_importer_ids(lockfile, manifests, prune_stale_importers);
    let Some(any_diverged) = any_importer_diverged(lockfile, &manifest_dependencies) else {
        return Drift::Resolve;
    };
    if !stale.is_empty() || any_diverged {
        Drift::Absorb(ImportersPlan {
            manifest_dependencies,
            stale,
            workspace_package_names: workspace_package_names(project_manifests),
            resolution_picks_lowest,
        })
    } else {
        Drift::Clean
    }
}

/// One manifest's declared dependencies keyed by alias, or `None` when an
/// alias cannot be parsed.
///
/// Later groups overwrite, so each alias ends at the group
/// `satisfies_package_manifest` expects it recorded under when it appears in
/// several: optional wins over prod, prod over dev.
fn manifest_dependency_map(manifest: &PackageManifest) -> Option<ManifestDependencies<'_>> {
    let mut dependencies = ManifestDependencies::default();
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
        for (name, specifier) in manifest.dependencies([group]) {
            dependencies.insert(PkgName::parse(name).ok()?, (specifier, group));
        }
    }
    Some(dependencies)
}

/// The importers no manifest claims any more.
fn stale_importer_ids(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
    prune_stale_importers: bool,
) -> Vec<String> {
    if !prune_stale_importers {
        return Vec::new();
    }
    let manifest_ids: HashSet<&str> =
        manifests.iter().map(|(importer_id, _)| importer_id.as_str()).collect();
    lockfile
        .importers
        .keys()
        .filter(|importer_id| !manifest_ids.contains(importer_id.as_str()))
        .cloned()
        .collect()
}

/// Whether any importer's record drifted from its manifest. `None` when one
/// of them can only be reconciled by resolving — bailing before the compose
/// clones the whole lockfile, since the apply would fail on it regardless.
fn any_importer_diverged(
    lockfile: &Lockfile,
    manifest_dependencies: &[(&String, &PackageManifest, ManifestDependencies<'_>)],
) -> Option<bool> {
    // `NeedsResolve` must win over `Absorbable` regardless of which importer
    // reports it, which the serial fold preserves.
    let mut any_diverged = false;
    for divergence in manifest_dependencies
        .par_iter()
        .map(|(importer_id, _, dependencies)| {
            importer_divergence(lockfile, importer_id, dependencies)
        })
        .collect::<Vec<_>>()
    {
        match divergence {
            ImporterDivergence::Clean => {}
            ImporterDivergence::Absorbable => any_diverged = true,
            ImporterDivergence::NeedsResolve => return None,
        }
    }
    Some(any_diverged)
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
    if !drop_stale_importers(candidate, plan, edits) {
        return false;
    }
    let Lockfile { snapshots, importers, time, .. } = candidate;
    let locked = LockedInputs { snapshots: snapshots.as_ref(), time: time.as_ref() };
    for (importer_id, manifest, manifest_dependencies) in &plan.manifest_dependencies {
        let entry = ImporterUpdate { importer_id, manifest, manifest_dependencies };
        if !apply_one_importer_update(importers, &entry, locked, plan, edits) {
            return false;
        }
    }
    true
}

/// Whether the importer's record differs from the manifest in a way the
/// update loop would act on: a changed specifier, a dependency recorded
/// under another group, a dependency the manifest no longer declares, or
/// a manifest entry the importer does not record (which the loop turns
/// into a fallback).
enum ImporterDivergence {
    Clean,
    Absorbable,
    /// A change [`apply_importers_update`] is certain to refuse, so the
    /// compose can go to the resolver without cloning the lockfile.
    NeedsResolve,
}

fn importer_divergence(
    lockfile: &Lockfile,
    importer_id: &str,
    manifest_dependencies: &ManifestDependencies<'_>,
) -> ImporterDivergence {
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return if manifest_dependencies.is_empty() {
            ImporterDivergence::Clean
        } else {
            ImporterDivergence::Absorbable
        };
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
    let mut diverged = recorded_but_undeclared;
    for (alias, (specifier, target)) in manifest_dependencies {
        match alias_divergence(importer, alias, (specifier, *target)) {
            AliasDivergence::Clean => {}
            AliasDivergence::Diverged => diverged = true,
            AliasDivergence::NeedsResolve => return ImporterDivergence::NeedsResolve,
        }
    }
    if diverged { ImporterDivergence::Absorbable } else { ImporterDivergence::Clean }
}

/// [`ImporterDivergence`] for one declared alias.
enum AliasDivergence {
    Clean,
    Diverged,
    NeedsResolve,
}

fn alias_divergence(
    importer: &ProjectSnapshot,
    alias: &PkgName,
    declared: (&str, DependencyGroup),
) -> AliasDivergence {
    let (specifier, target) = declared;
    let Some((recorded_in, dependency)) = importer_dependency(importer, alias) else {
        return AliasDivergence::Diverged;
    };
    if dependency.specifier != specifier {
        // The same conditions [`apply_importers_update`] holds a
        // changed specifier to before it consults the locked
        // versions; a specifier they reject (a `workspace:` range
        // above all) can only resolve.
        if Range::parse(specifier).is_err()
            || dependency
                .version
                .ver_peer()
                .and_then(|ver_peer| ver_peer.version_semver())
                .is_none()
        {
            return AliasDivergence::NeedsResolve;
        }
        return AliasDivergence::Diverged;
    }
    if recorded_in == target { AliasDivergence::Clean } else { AliasDivergence::Diverged }
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
/// declares, recording the severed edges in `edits`.
fn remove_dependencies_absent_from(
    importer: &mut ProjectSnapshot,
    manifest_dependencies: &ManifestDependencies<'_>,
    edits: &mut GraphEdits,
) {
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
        group.retain(|alias, dependency| {
            if declared(alias) {
                return true;
            }
            edits.dropped.record(alias, dependency);
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
