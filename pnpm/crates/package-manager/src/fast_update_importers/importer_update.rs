use super::{
    ImportersPlan, ManifestDependencies, importer_dependency, importer_dependency_mut,
    importer_group,
    locked_versions::{
        LockedPick, LockedSnapshots, is_linked_from_a_survivor,
        locked_version_resolution_would_pick,
    },
    move_dependency, remove_dependencies_absent_from,
};
use crate::{
    dependencies_graph_to_lockfile::manifest_publish_config, fast_update_lockfile::GraphEdits,
    fast_update_settings::is_directory_dependency,
};
use node_semver::{Range, Version};
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, PackageKey, PkgName, ProjectSnapshot, ResolvedDependencySpec,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::{BTreeMap, HashMap};

/// The lockfile halves an importer edge is replayed against.
#[derive(Clone, Copy)]
pub(super) struct LockedInputs<'a> {
    pub(super) snapshots: Option<&'a LockedSnapshots>,
    pub(super) time: Option<&'a BTreeMap<String, String>>,
}
/// One importer's manifest and the dependencies it declares.
pub(super) struct ImporterUpdate<'a, 'manifest> {
    pub(super) importer_id: &'a String,
    pub(super) manifest: &'manifest PackageManifest,
    pub(super) manifest_dependencies: &'a ManifestDependencies<'manifest>,
}
/// Drop the importers no project claims any more. A project that is gone
/// while something still links to it is a broken workspace, which only the
/// resolver may report.
pub(super) fn drop_stale_importers(
    candidate: &mut Lockfile,
    plan: &ImportersPlan<'_, '_>,
    edits: &mut GraphEdits,
) -> bool {
    if plan
        .stale
        .iter()
        .any(|importer_id| is_linked_from_a_survivor(candidate, importer_id, &plan.stale))
    {
        return false;
    }
    for importer_id in &plan.stale {
        if let Some(importer) = candidate.importers.remove(importer_id) {
            record_dropped_importer_edges(&importer, edits);
        }
    }
    true
}
pub(super) fn record_dropped_importer_edges(importer: &ProjectSnapshot, edits: &mut GraphEdits) {
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
pub(super) fn apply_one_importer_update(
    importers: &mut HashMap<String, ProjectSnapshot>,
    entry: &ImporterUpdate<'_, '_>,
    locked: LockedInputs<'_>,
    plan: &ImportersPlan<'_, '_>,
    edits: &mut GraphEdits,
) -> bool {
    let ImporterUpdate { importer_id, manifest, manifest_dependencies } = *entry;
    let records_nothing = importers.get(importer_id.as_str()).is_none_or(records_no_dependencies);
    if records_nothing && !manifest_dependencies.is_empty() {
        let Some(new_importer) =
            importer_from_locked_versions(locked.snapshots, manifest, manifest_dependencies, plan)
        else {
            return false;
        };
        importers.insert(importer_id.clone(), new_importer);
        // The only edit that adds reachability, so a package that until
        // now only optional dependencies reached can have stopped being
        // optional.
        edits.optional_flags_are_stale = true;
        return true;
    }
    let Some(importer) = importers.get_mut(importer_id.as_str()) else {
        return false;
    };
    for (alias, (specifier, target)) in manifest_dependencies {
        if !apply_importer_edge(importer, alias, (specifier, *target), locked, plan, edits) {
            return false;
        }
    }
    remove_dependencies_absent_from(importer, manifest_dependencies, edits);
    true
}
pub(super) fn apply_importer_edge(
    importer: &mut ProjectSnapshot,
    alias: &PkgName,
    declared: (&str, DependencyGroup),
    locked: LockedInputs<'_>,
    plan: &ImportersPlan<'_, '_>,
    edits: &mut GraphEdits,
) -> bool {
    let (specifier, target) = declared;
    if importer_dependency(importer, alias).is_none() {
        return add_importer_edge(
            importer,
            alias,
            (specifier, target),
            locked.snapshots,
            locked.time,
            plan,
            edits,
        );
    }
    let dependency = importer_dependency_mut(importer, alias).expect("looked up just above");
    if dependency.specifier != specifier
        && !retarget_importer_dependency(dependency, alias, specifier, locked, plan, edits)
    {
        return false;
    }
    if let Some(source) = move_dependency(importer, alias, target) {
        edits.optional_flags_are_stale |=
            source == DependencyGroup::Optional || target == DependencyGroup::Optional;
    }
    true
}
/// Move a declared dependency onto the version its changed specifier resolves
/// to. Safe without resolving because the target version is already in the
/// lockfile, subtree and all.
pub(super) fn retarget_importer_dependency(
    dependency: &mut ResolvedDependencySpec,
    alias: &PkgName,
    specifier: &str,
    locked: LockedInputs<'_>,
    plan: &ImportersPlan<'_, '_>,
    edits: &mut GraphEdits,
) -> bool {
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
        locked.snapshots,
        alias,
        &range,
        plan.resolution_picks_lowest,
    ) else {
        return false;
    };
    let moves = wanted.version != *version;
    let recorded = PackageKey::new(alias.clone(), ver_peer.clone());
    if !retarget_names_a_snapshot(locked.snapshots, &wanted, moves, &recorded) {
        return false;
    }
    if moves {
        if recorded.suffix.peer() != "" {
            return false;
        }
        let Ok(moved) = wanted.version.to_string().parse() else {
            return false;
        };
        edits.dropped.record(alias, &*dependency);
        dependency.version = ImporterDepVersion::Regular(moved);
    }
    dependency.specifier = specifier.to_string();
    true
}
/// Whether the record a retarget leaves behind names a snapshot the
/// lockfile holds.
///
/// A move writes the bare version, so the lockfile has to hold that
/// version with no peers resolved; which of several peer variants the
/// edge would take instead is the resolver's call. An edge that stays put
/// keeps the record it already carries, which is looked up rather than
/// assumed: nothing guarantees the lockfile on disk is self-consistent.
pub(super) fn retarget_names_a_snapshot(
    snapshots: Option<&LockedSnapshots>,
    wanted: &LockedPick,
    moves: bool,
    recorded: &PackageKey,
) -> bool {
    if moves {
        return !wanted.peer_suffixed;
    }
    snapshots.is_some_and(|snapshots| snapshots.contains_key(recorded))
}
/// Whether the lockfile records no dependency of this project — the
/// shape a project it has never seen arrives in, alongside an absent
/// entry.
pub(super) fn records_no_dependencies(importer: &ProjectSnapshot) -> bool {
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
/// specifier is not a semver range, one no locked version satisfies, and
/// one the lockfile holds only as a peer variant.
pub(super) fn importer_from_locked_versions(
    snapshots: Option<&LockedSnapshots>,
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
        let pick = locked_version_resolution_would_pick(
            snapshots,
            alias,
            &range,
            plan.resolution_picks_lowest,
        )?;
        if pick.peer_suffixed {
            return None;
        }
        let dependency = ResolvedDependencySpec {
            specifier: (*specifier).to_string(),
            version: ImporterDepVersion::Regular(pick.version.to_string().parse().ok()?),
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
/// and its subtree are already recorded, and the record this writes carries
/// no peer suffix, so a version the lockfile only holds as a peer variant is
/// left to the resolver.
///
/// `false` leaves the caller on the full-resolution path.
pub(super) fn add_importer_edge(
    importer: &mut ProjectSnapshot,
    alias: &PkgName,
    declared: (&str, DependencyGroup),
    snapshots: Option<&LockedSnapshots>,
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
    let Some(wanted) = locked_version_resolution_would_pick(
        snapshots,
        alias,
        &range,
        plan.resolution_picks_lowest,
    ) else {
        return false;
    };
    if wanted.peer_suffixed {
        return false;
    }
    let wanted = wanted.version;
    // `time` carries a publish date per direct dependency, and only a
    // resolution can look up the one for a package this promotes into that
    // position.
    if time.is_some_and(|time| !time.contains_key(&format!("{alias}@{wanted}"))) {
        return false;
    }
    insert_importer_edge(importer, alias, specifier, target, &wanted, edits)
}
pub(super) fn insert_importer_edge(
    importer: &mut ProjectSnapshot,
    alias: &PkgName,
    specifier: &str,
    target: DependencyGroup,
    wanted: &Version,
    edits: &mut GraphEdits,
) -> bool {
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
