use crate::{
    dependencies_graph_to_lockfile::manifest_publish_config,
    fast_update_compose::Drift,
    fast_update_lockfile::GraphEdits,
    fast_update_settings::{is_directory_dependency, workspace_package_names},
};
use node_semver::{Range, Version};
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, PackageKey, PkgName, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

/// The lockfile's `packages:` block, which an absorbed edge reads the
/// version it points at out of.
type LockedPackages = HashMap<PackageKey, pnpm_lockfile::PackageMetadata>;

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
    let manifest_dependencies: Result<
        Vec<(&String, &PackageManifest, ManifestDependencies<'_>)>,
        (),
    > = manifests
        .par_iter()
        .map(|(importer_id, manifest)| {
            // Later groups overwrite, so each alias ends at the group
            // `satisfies_package_manifest` expects it recorded under when it
            // appears in several: optional wins over prod, prod over dev.
            let mut dependencies = ManifestDependencies::default();
            for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
                for (name, specifier) in manifest.dependencies([group]) {
                    let Ok(name) = PkgName::parse(name) else {
                        return Err(());
                    };
                    dependencies.insert(name, (specifier, group));
                }
            }
            Ok((importer_id, *manifest, dependencies))
        })
        .collect();
    let Ok(manifest_dependencies) = manifest_dependencies else {
        return Drift::Resolve;
    };
    let stale: Vec<String> = if prune_stale_importers {
        let manifest_ids: HashSet<&str> =
            manifests.iter().map(|(importer_id, _)| importer_id.as_str()).collect();
        lockfile
            .importers
            .keys()
            .filter(|importer_id| !manifest_ids.contains(importer_id.as_str()))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    // `Resolve` must win over `Absorbable` regardless of which importer
    // reports it, which the fold preserves.
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
            // Bail before the compose clones the whole lockfile: the
            // apply would fail on this importer regardless.
            ImporterDivergence::NeedsResolve => return Drift::Resolve,
        }
    }
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
    // A project that is gone while something still links to it is a broken
    // workspace, which only the resolver may report.
    if plan
        .stale
        .iter()
        .any(|importer_id| is_linked_from_a_survivor(candidate, importer_id, &plan.stale))
    {
        return false;
    }
    for importer_id in &plan.stale {
        if let Some(importer) = candidate.importers.remove(importer_id) {
            for group in [
                importer.dependencies.as_ref(),
                importer.dev_dependencies.as_ref(),
                importer.optional_dependencies.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                for (alias, dependency) in group {
                    edits.dropped.record(alias, dependency);
                }
            }
        }
    }
    let Lockfile { packages, importers, time, .. } = candidate;
    for (importer_id, manifest, manifest_dependencies) in &plan.manifest_dependencies {
        let records_nothing =
            importers.get(importer_id.as_str()).is_none_or(records_no_dependencies);
        if records_nothing && !manifest_dependencies.is_empty() {
            let Some(new_importer) = importer_from_locked_versions(
                packages.as_ref(),
                manifest,
                manifest_dependencies,
                plan,
            ) else {
                return false;
            };
            importers.insert((*importer_id).clone(), new_importer);
            // The only edit that adds reachability, so a package that until
            // now only optional dependencies reached can have stopped being
            // optional.
            edits.optional_flags_are_stale = true;
            continue;
        }
        let Some(importer) = importers.get_mut(importer_id.as_str()) else {
            return false;
        };
        for (alias, (specifier, target)) in manifest_dependencies {
            if importer_dependency(importer, alias).is_none() {
                if !add_importer_edge(
                    importer,
                    alias,
                    (specifier, *target),
                    packages.as_ref(),
                    time.as_ref(),
                    plan,
                    edits,
                ) {
                    return false;
                }
                continue;
            }
            let dependency =
                importer_dependency_mut(importer, alias).expect("looked up just above");
            let specifier_changed = dependency.specifier != *specifier;
            if specifier_changed {
                let Ok(range) = Range::parse(specifier) else {
                    return false;
                };
                let Some(ver_peer) = dependency.version.ver_peer() else {
                    return false;
                };
                let Some(version) = ver_peer.version_semver() else {
                    return false;
                };
                let Some(wanted) = locked_version_resolution_would_pick(
                    packages.as_ref(),
                    alias,
                    &range,
                    plan.resolution_picks_lowest,
                ) else {
                    return false;
                };
                if wanted != *version {
                    // Safe without resolving because the target version is
                    // already in the lockfile, subtree and all.
                    if ver_peer.peer() != "" {
                        return false;
                    }
                    let Ok(moved) = wanted.to_string().parse() else {
                        return false;
                    };
                    edits.dropped.record(alias, &*dependency);
                    dependency.version = ImporterDepVersion::Regular(moved);
                }
                dependency.specifier = (*specifier).to_string();
            }
            if let Some(source) = move_dependency(importer, alias, *target) {
                edits.optional_flags_are_stale |=
                    source == DependencyGroup::Optional || *target == DependencyGroup::Optional;
            }
        }
        remove_dependencies_absent_from(importer, manifest_dependencies, edits);
    }
    true
}

/// Whether the lockfile records no dependency of this project — the
/// shape a project it has never seen arrives in, alongside an absent
/// entry.
fn records_no_dependencies(importer: &ProjectSnapshot) -> bool {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .all(HashMap::is_empty)
}

/// A project's whole importer entry, built from the versions the
/// lockfile already holds.
///
/// `None` when a declared dependency needs the resolver: one that
/// resolves to a directory rather than to a registry version, one whose
/// specifier is not a semver range, and one no locked version satisfies.
fn importer_from_locked_versions(
    packages: Option<&HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    manifest: &PackageManifest,
    manifest_dependencies: &ManifestDependencies<'_>,
    plan: &ImportersPlan<'_, '_>,
) -> Option<ProjectSnapshot> {
    let mut importer = ProjectSnapshot::default();
    let mut specifiers = HashMap::new();
    for (alias, (specifier, group)) in manifest_dependencies {
        if is_directory_dependency(&alias.to_string(), specifier, &plan.workspace_package_names) {
            return None;
        }
        let range = Range::parse(specifier).ok()?;
        let version = locked_version_resolution_would_pick(
            packages,
            alias,
            &range,
            plan.resolution_picks_lowest,
        )?;
        let dependency = ResolvedDependencySpec {
            specifier: (*specifier).to_string(),
            version: ImporterDepVersion::Regular(version.to_string().parse().ok()?),
        };
        importer_group(&mut importer, *group)
            .get_or_insert_default()
            .insert(alias.clone(), dependency);
        specifiers.insert(alias.to_string(), (*specifier).to_string());
    }
    importer.specifiers = Some(specifiers);
    importer.dependencies_meta = manifest.value().get("dependenciesMeta").cloned();
    (importer.publish_directory, importer.link_directory) = manifest_publish_config(manifest);
    Some(importer)
}

/// Record `alias` as a direct dependency of `importer` at the version the
/// lockfile already holds for it, under the group the manifest declares it
/// in.
///
/// Safe without resolving for the same reason a moved range is: the version
/// and its subtree are already recorded, and a subtree that resolved a peer
/// from outside itself would have left `alias` peer-suffixed, which
/// [`locked_version_resolution_would_pick`] refuses.
///
/// `false` leaves the caller on the full-resolution path.
fn add_importer_edge(
    importer: &mut ProjectSnapshot,
    alias: &PkgName,
    declared: (&str, DependencyGroup),
    packages: Option<&LockedPackages>,
    time: Option<&BTreeMap<String, String>>,
    plan: &ImportersPlan<'_, '_>,
    edits: &mut GraphEdits,
) -> bool {
    let (specifier, target) = declared;
    // A recorded specifier with nothing to point at is a lockfile only the
    // resolver can make sense of.
    if importer
        .specifiers
        .as_ref()
        .is_some_and(|specifiers| specifiers.contains_key(&alias.to_string()))
    {
        return false;
    }
    if is_directory_dependency(&alias.to_string(), specifier, &plan.workspace_package_names) {
        return false;
    }
    let Ok(range) = Range::parse(specifier) else {
        return false;
    };
    let Some(wanted) =
        locked_version_resolution_would_pick(packages, alias, &range, plan.resolution_picks_lowest)
    else {
        return false;
    };
    // `time` carries a publish date per direct dependency, and only a
    // resolution can look up the one for a package this promotes into that
    // position.
    if time.is_some_and(|time| !time.contains_key(&format!("{alias}@{wanted}"))) {
        return false;
    }
    let Ok(version) = wanted.to_string().parse() else {
        return false;
    };
    importer_group(importer, target).get_or_insert_default().insert(
        alias.clone(),
        ResolvedDependencySpec {
            specifier: specifier.to_string(),
            version: ImporterDepVersion::Regular(version),
        },
    );
    if let Some(specifiers) = importer.specifiers.as_mut() {
        specifiers.insert(alias.to_string(), specifier.to_string());
    }
    // A path that does not run through `optionalDependencies` clears the
    // `optional` flag of everything the new edge reaches.
    edits.optional_flags_are_stale |= target != DependencyGroup::Optional;
    true
}

/// Whether an importer that survives the prune links to `importer_id`.
fn is_linked_from_a_survivor(lockfile: &Lockfile, importer_id: &str, stale: &[String]) -> bool {
    lockfile.importers.iter().any(|(survivor_id, importer)| {
        if stale.iter().any(|id| id == survivor_id) {
            return false;
        }
        [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|group| {
            group.values().any(|spec| {
                spec.version
                    .as_link_target()
                    .is_some_and(|target| link_resolves_to(survivor_id, target, importer_id))
            })
        })
    })
}

/// Whether `target`, a `link:` path relative to `from`'s directory,
/// names the importer `importer_id`.
fn link_resolves_to(from: &str, target: &str, importer_id: &str) -> bool {
    let mut segments: Vec<&str> = if from == "." { Vec::new() } else { from.split('/').collect() };
    for part in target.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                if segments.pop().is_none() {
                    return false;
                }
            }
            other => segments.push(other),
        }
    }
    segments.join("/") == importer_id
}

/// The version resolution would settle on for `alias` under `range`: the
/// highest version of it the lockfile already holds that satisfies the
/// range.
///
/// Resolution prefers a version already in the graph over a higher one
/// from the registry, so reusing what is present is what it would
/// record — for a widened range as much as for one the locked version
/// cannot satisfy at all.
///
/// `None` when the pick cannot be read off the lockfile:
///
/// - nothing present satisfies, so only the resolver can fetch a version;
/// - `resolution_picks_lowest` and more than one locked version
///   satisfies. Those resolution modes take the lowest preferred version
///   for a direct dependency, but only when the run leaves the manifest
///   alone, so which end of the range applies is not a property of the
///   lockfile;
/// - the alias appears under a key this cannot turn back into a plain
///   reference: a peer-suffixed one, where picking a variant would be a
///   guess, or a registry-qualified one, whose semver only pins a version
///   within its named registry.
pub(crate) fn locked_version_resolution_would_pick(
    packages: Option<&LockedPackages>,
    alias: &PkgName,
    range: &Range,
    resolution_picks_lowest: bool,
) -> Option<Version> {
    let mut highest: Option<Version> = None;
    let mut satisfying = 0_usize;
    for key in packages?.keys() {
        if &key.name != alias {
            continue;
        }
        if !key.suffix.peer().is_empty() || key.suffix.registry_qualified().is_some() {
            return None;
        }
        let Some(version) = key.suffix.version_semver() else { continue };
        if version.satisfies(range) {
            satisfying += 1;
            if highest.as_ref().is_none_or(|best| version > best) {
                highest = Some(version.clone());
            }
        }
    }
    if satisfying > 1 && resolution_picks_lowest {
        return None;
    }
    highest
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
        let Some((recorded_in, dependency)) = importer_dependency(importer, alias) else {
            diverged = true;
            continue;
        };
        if dependency.specifier != *specifier {
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
                return ImporterDivergence::NeedsResolve;
            }
            diverged = true;
        }
        if recorded_in != *target {
            diverged = true;
        }
    }
    if diverged { ImporterDivergence::Absorbable } else { ImporterDivergence::Clean }
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
