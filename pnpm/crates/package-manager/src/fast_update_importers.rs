use crate::{
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
type ManifestDependencies<'manifest> = HashMap<PkgName, (&'manifest str, DependencyGroup)>;

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
/// cloning anything. [`Drift::Resolve`] when an alias cannot be parsed,
/// which the resolver reports.
pub(crate) fn detect_importers_drift<'a, 'manifest>(
    lockfile: &Lockfile,
    manifests: &'a [(String, &'manifest PackageManifest)],
    project_manifests: &[(PathBuf, &PackageManifest)],
    prune_stale_importers: bool,
    resolution_picks_lowest: bool,
) -> Drift<ImportersPlan<'a, 'manifest>> {
    let mut manifest_dependencies: Vec<(&String, &PackageManifest, ManifestDependencies<'_>)> =
        Vec::new();
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
        manifest_dependencies.push((importer_id, *manifest, dependencies));
    }
    let stale: Vec<String> = if prune_stale_importers {
        lockfile
            .importers
            .keys()
            .filter(|importer_id| !manifests.iter().any(|(id, _)| id == *importer_id))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    if !stale.is_empty()
        || manifest_dependencies.iter().any(|(importer_id, _, dependencies)| {
            importer_diverges(lockfile, importer_id, dependencies)
        })
    {
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
    importer.publish_directory = manifest
        .value()
        .get("publishConfig")
        .and_then(|publish_config| publish_config.get("directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);
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
fn importer_diverges(
    lockfile: &Lockfile,
    importer_id: &str,
    manifest_dependencies: &ManifestDependencies<'_>,
) -> bool {
    let Some(importer) = lockfile.importers.get(importer_id) else {
        return !manifest_dependencies.is_empty();
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
