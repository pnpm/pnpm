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

/// The lockfile's `snapshots:` block, which an absorbed edge reads the
/// version it points at out of. Only its keys carry a peer suffix; the
/// `packages:` key of a package that was resolved against a peer is the
/// bare `name@version`, which names no snapshot an importer could link.
type LockedSnapshots = HashMap<PackageKey, pnpm_lockfile::SnapshotEntry>;

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

/// The lockfile halves an importer edge is replayed against.
#[derive(Clone, Copy)]
struct LockedInputs<'a> {
    snapshots: Option<&'a LockedSnapshots>,
    time: Option<&'a BTreeMap<String, String>>,
}

/// One importer's manifest and the dependencies it declares.
struct ImporterUpdate<'a, 'manifest> {
    importer_id: &'a String,
    manifest: &'manifest PackageManifest,
    manifest_dependencies: &'a ManifestDependencies<'manifest>,
}

/// Drop the importers no project claims any more. A project that is gone
/// while something still links to it is a broken workspace, which only the
/// resolver may report.
fn drop_stale_importers(
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

fn record_dropped_importer_edges(importer: &ProjectSnapshot, edits: &mut GraphEdits) {
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

fn apply_one_importer_update(
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

fn apply_importer_edge(
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
fn retarget_importer_dependency(
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
fn retarget_names_a_snapshot(
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
/// specifier is not a semver range, one no locked version satisfies, and
/// one the lockfile holds only as a peer variant.
fn importer_from_locked_versions(
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
fn add_importer_edge(
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
/// - the alias appears under a registry-qualified key, whose semver only
///   pins a version within its named registry.
pub(crate) fn locked_version_resolution_would_pick(
    snapshots: Option<&LockedSnapshots>,
    alias: &PkgName,
    range: &Range,
    resolution_picks_lowest: bool,
) -> Option<LockedPick> {
    let mut highest: Option<LockedPick> = None;
    let mut several_versions_satisfy = false;
    for key in snapshots?.keys() {
        match locked_candidate(key, alias, range) {
            LockedCandidate::Unsupported => return None,
            LockedCandidate::Ignored => {}
            LockedCandidate::Satisfying(pick) => {
                several_versions_satisfy |= keep_the_higher_version(&mut highest, pick);
            }
        }
    }
    if resolution_picks_lowest && several_versions_satisfy {
        return None;
    }
    highest
}

/// Fold one satisfying candidate into the running pick, and report whether
/// it named a version other than the one already held.
///
/// A version several snapshots name is a peer variant as soon as one of
/// them says so.
fn keep_the_higher_version(highest: &mut Option<LockedPick>, pick: LockedPick) -> bool {
    let Some(best) = highest else {
        *highest = Some(pick);
        return false;
    };
    if best.version == pick.version {
        best.peer_suffixed |= pick.peer_suffixed;
        return false;
    }
    if pick.version > best.version {
        *best = pick;
    }
    true
}

/// The version [`locked_version_resolution_would_pick`] settles on.
pub(crate) struct LockedPick {
    pub(crate) version: Version,
    /// Whether a snapshot names this version with a peer suffix. A record
    /// written at the bare version would then name a snapshot the lockfile
    /// does not hold, and which variant it should name instead is the
    /// resolver's call.
    pub(crate) peer_suffixed: bool,
}

/// What one locked snapshot key contributes to the version pick.
enum LockedCandidate {
    /// The key names another package, or a version the range rejects.
    Ignored,
    Satisfying(LockedPick),
    /// A key shape the fast path cannot reason about.
    Unsupported,
}

fn locked_candidate(key: &PackageKey, alias: &PkgName, range: &Range) -> LockedCandidate {
    if &key.name != alias {
        return LockedCandidate::Ignored;
    }
    if key.suffix.registry_qualified().is_some() {
        return LockedCandidate::Unsupported;
    }
    let Some(version) = key.suffix.version_semver() else {
        return LockedCandidate::Ignored;
    };
    if version.satisfies(range) {
        return LockedCandidate::Satisfying(LockedPick {
            version: version.clone(),
            peer_suffixed: !key.suffix.peer().is_empty(),
        });
    }
    LockedCandidate::Ignored
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
