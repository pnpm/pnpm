//! Filter a wanted lockfile down to the "current" shape pacquet
//! writes under `<virtual_store_dir>/lock.yaml`.
//!
//! Rather than re-running the engine + `supportedArchitectures` +
//! `skipped` checks at filter time, reuse the [`SkippedSnapshots`]
//! set produced during install. The full set filters materialized
//! snapshots; the package metadata map only drops user exclusions
//! such as `--no-optional` and `--no-runtime`.
//!
//! The output drives the **next** install's diff. Without this
//! filter, pacquet's current lockfile recorded every snapshot the
//! resolver imagined, including ones that `--no-optional` or
//! installability dropped — so a follow-up install would think
//! those slots were already on disk and skip work that should
//! actually run.

use std::collections::{HashMap, HashSet, VecDeque};

use pacquet_lockfile::{
    Lockfile, PackageKey, PkgName, ProjectSnapshot, ResolvedDependencyMap, SnapshotDepRef,
    SnapshotEntry,
};
use pacquet_modules_yaml::IncludedDependencies;

use crate::SkippedSnapshots;

/// Build the "current lockfile" shape from the wanted lockfile by
/// applying the install-time `include` set and skip set.
///
/// Importers lose dep maps whose `include` flag is false; importer
/// `optionalDependencies` lose entries whose resolved snapshot got
/// skipped; the snapshot map is pruned to the transitive closure
/// reachable from the surviving importer roots. The package metadata
/// map preserves installability-skipped entries, matching pnpm's
/// current-lockfile shape while `.modules.yaml.skipped` records the
/// materialization skip.
#[must_use]
pub fn filter_lockfile_for_current(
    lockfile: &Lockfile,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> Lockfile {
    let reachable = collect_reachable(&lockfile.importers, lockfile.snapshots.as_ref(), |key| {
        skipped.contains(key)
    });
    let metadata_reachable =
        collect_reachable(&lockfile.importers, lockfile.snapshots.as_ref(), |key| {
            skipped.contains_optional_excluded(key)
        });

    // Reachable metadata keys: snapshot keys without the peer suffix.
    // The `packages:` map is keyed by `metadata_key` (e.g.
    // `react-dom@17.0.2`), not by snapshot key
    // (e.g. `react-dom@17.0.2(react@17.0.2)`); a single metadata
    // entry can back multiple peer-variant snapshots. Compute the
    // union of `without_peer()` keys so peer-variant survivors keep
    // their shared metadata row.
    let mut reachable_metadata: HashSet<PackageKey> = HashSet::new();
    for snap_key in &metadata_reachable {
        reachable_metadata.insert(snap_key.without_peer());
    }

    let importers = lockfile
        .importers
        .iter()
        .map(|(id, imp)| (id.clone(), filter_importer(imp, included, &reachable)))
        .collect();

    let snapshots = lockfile.snapshots.as_ref().map(|snapshots| {
        snapshots
            .iter()
            .filter(|(k, _)| reachable.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>()
    });

    let packages = lockfile.packages.as_ref().map(|packages| {
        packages
            .iter()
            .filter(|(k, _)| reachable_metadata.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<HashMap<_, _>>()
    });

    Lockfile {
        lockfile_version: lockfile.lockfile_version,
        settings: lockfile.settings.clone(),
        // The current lockfile is a filtered view of the wanted one;
        // catalog snapshots carry over verbatim.
        catalogs: lockfile.catalogs.clone(),
        overrides: lockfile.overrides.clone(),
        package_extensions_checksum: lockfile.package_extensions_checksum.clone(),
        // Carried over verbatim — the current lockfile is a filtered
        // view of the wanted one, so its pnpmfile checksum round-trips.
        pnpmfile_checksum: lockfile.pnpmfile_checksum.clone(),
        // Preserve the wanted lockfile's `ignored_optional_dependencies`
        // verbatim — the current lockfile is a filtered view of the
        // wanted one, and a future drift check between this recorded
        // set and the next install's `Config` value relies on the
        // round-trip. Slice 7 wire-up.
        ignored_optional_dependencies: lockfile.ignored_optional_dependencies.clone(),
        // Carried over verbatim: the current lockfile is a filtered
        // view of the wanted one, so the recorded patch hashes survive
        // the round-trip into `node_modules/.pnpm/lock.yaml`.
        patched_dependencies: lockfile.patched_dependencies.clone(),
        importers,
        packages,
        snapshots,
    }
}

/// Per-importer filter: drop dep maps whose `include` flag is
/// false; further trim `optional_dependencies` to entries whose
/// resolved snapshot survived the reachability walk.
///
/// Two steps: first clear the excluded dep sections, then
/// post-filter `optionalDependencies` against the surviving
/// packages set.
fn filter_importer(
    importer: &ProjectSnapshot,
    included: IncludedDependencies,
    reachable: &HashSet<PackageKey>,
) -> ProjectSnapshot {
    let mut out = importer.clone();
    if !included.dependencies {
        out.dependencies = None;
    }
    if !included.dev_dependencies {
        out.dev_dependencies = None;
    }
    if !included.optional_dependencies {
        out.optional_dependencies = None;
    } else if let Some(opt) = out.optional_dependencies.as_mut() {
        retain_reachable(opt, reachable);
    }
    out
}

/// Drop importer-level optional-dep entries whose resolved
/// snapshot key isn't in `reachable`. `link:` entries (workspace
/// siblings) survive — they don't live in the snapshot graph.
fn retain_reachable(map: &mut ResolvedDependencyMap, reachable: &HashSet<PackageKey>) {
    map.retain(|name, spec| {
        let Some(key) = spec.version.resolved_key(name) else {
            // Workspace `link:<path>` — no snapshot to check.
            return true;
        };
        reachable.contains(&key)
    });
}

/// BFS the snapshot graph from every importer-root dep, skipping keys
/// selected by `should_skip`. Returns the set of snapshot keys that
/// survive.
fn collect_reachable<ShouldSkip>(
    importers: &HashMap<String, ProjectSnapshot>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    should_skip: ShouldSkip,
) -> HashSet<PackageKey>
where
    ShouldSkip: Fn(&PackageKey) -> bool,
{
    let Some(snapshots) = snapshots else { return HashSet::new() };

    let mut reachable: HashSet<PackageKey> = HashSet::new();
    let mut queue: VecDeque<PackageKey> = VecDeque::new();

    // Seed the queue from every importer-level dep map. The
    // `include` filter isn't applied here on purpose — all three
    // maps are walked unconditionally at this stage, and only the
    // importer-level *output* clears the maps that were excluded. A
    // snapshot reachable through any importer map stays in the
    // snapshot graph as long as it's not in `skipped`; the
    // per-importer clearing happens separately in [`filter_importer`].
    for importer in importers.values() {
        for map in [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for (name, spec) in map {
                let Some(key) = spec.version.resolved_key(name) else { continue };
                if should_skip(&key) {
                    continue;
                }
                if snapshots.contains_key(&key) {
                    queue.push_back(key);
                }
            }
        }
    }

    while let Some(key) = queue.pop_front() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(snap) = snapshots.get(&key) else { continue };
        for dep_map in
            [snap.dependencies.as_ref(), snap.optional_dependencies.as_ref()].into_iter().flatten()
        {
            for (alias, dep_ref) in dep_map {
                if let Some(child) = resolve_child(alias, dep_ref, snapshots, &should_skip) {
                    queue.push_back(child);
                }
            }
        }
    }

    reachable
}

/// Resolve a snapshot child to its `PackageKey`, dropping it if the
/// target should be skipped or is absent from the snapshot map.
fn resolve_child<ShouldSkip>(
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    should_skip: &ShouldSkip,
) -> Option<PackageKey>
where
    ShouldSkip: Fn(&PackageKey) -> bool,
{
    // `link:` deps live outside the virtual store and have no
    // snapshot to reach — they aren't part of the reachable-snapshot
    // graph this helper computes.
    let resolved = dep_ref.resolve(alias)?;
    if should_skip(&resolved) {
        return None;
    }
    snapshots.contains_key(&resolved).then_some(resolved)
}

#[cfg(test)]
mod tests;
