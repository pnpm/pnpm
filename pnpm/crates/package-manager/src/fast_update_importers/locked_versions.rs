use node_semver::{Range, Version};
use pnpm_lockfile::{Lockfile, PackageKey, PkgName};
use pnpm_package_manifest::DependencyGroup;
use std::collections::HashMap;

/// The lockfile's `snapshots:` block, which an absorbed edge reads the
/// version it points at out of. Only its keys carry a peer suffix; the
/// `packages:` key of a package that was resolved against a peer is the
/// bare `name@version`, which names no snapshot an importer could link.
pub(super) type LockedSnapshots = HashMap<PackageKey, pnpm_lockfile::SnapshotEntry>;
/// Whether an importer that survives the prune links to `importer_id`.
pub(super) fn is_linked_from_a_survivor(
    lockfile: &Lockfile,
    importer_id: &str,
    stale: &[String],
) -> bool {
    for (survivor_id, importer) in &lockfile.importers {
        if stale.contains(survivor_id) {
            continue;
        }
        for (_, spec) in importer.dependencies_by_groups([
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
        ]) {
            let Some(target) = spec.version.as_link_target() else {
                continue;
            };
            if link_resolves_to(survivor_id, target, importer_id) {
                return true;
            }
        }
    }
    false
}
/// Whether `target`, a `link:` path relative to `from`'s directory,
/// names the importer `importer_id`.
pub(super) fn link_resolves_to(from: &str, target: &str, importer_id: &str) -> bool {
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
pub(super) fn keep_the_higher_version(highest: &mut Option<LockedPick>, pick: LockedPick) -> bool {
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
pub(super) enum LockedCandidate {
    /// The key names another package, or a version the range rejects.
    Ignored,
    Satisfying(LockedPick),
    /// A key shape the fast path cannot reason about.
    Unsupported,
}
pub(super) fn locked_candidate(
    key: &PackageKey,
    alias: &PkgName,
    range: &Range,
) -> LockedCandidate {
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
