use crate::fast_update_compose::Drift;
use pnpm_deps_path::{index_of_dep_path_suffix, remove_suffix};
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, PackageKey, ProjectSnapshot, ResolvedDependencyMap,
    SnapshotDepRef,
};
use pnpm_patching::{
    PatchGroupRecord, PatchInput, all_patch_keys, get_patch_info, group_patched_dependencies,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Where a package's snapshot key moves to when its patch changes.
type Rekeys = HashMap<PackageKey, PackageKey>;

/// Resolution never reads a patch: it appends the patch file's hash to
/// an already-resolved package id, so the set of packages and versions
/// is the same either way. Only the affected packages' `snapshots:`
/// keys and the references pointing at them move, which is a rewrite of
/// the loaded lockfile rather than a re-resolve. `packages:` is keyed
/// without the patch hash, so it stays as it is.
///
/// The plan holds the configured patches [`apply_patched_update`] moves
/// the lockfile to: the hashes to record and their resolver-shaped
/// grouping.
pub(crate) struct PatchedPlan {
    current: BTreeMap<String, String>,
    groups: PatchGroupRecord,
}

/// Whether `patchedDependencies` drifted from what the lockfile
/// records, against the hashes of the configured patch files.
/// [`Drift::Resolve`] when a key's version segment parses as neither a
/// version nor a range — the resolver reports that.
pub(crate) fn detect_patched_drift(
    lockfile: &Lockfile,
    hashes: Option<&BTreeMap<String, String>>,
) -> Drift<PatchedPlan> {
    let empty = BTreeMap::new();
    let recorded = lockfile.patched_dependencies.as_ref().unwrap_or(&empty);
    let current = hashes.unwrap_or(&empty);
    if recorded == current {
        return Drift::Clean;
    }
    match groups_from_hashes(current) {
        Some(groups) => Drift::Absorb(PatchedPlan { current: current.clone(), groups }),
        None => Drift::Resolve,
    }
}

/// Rekey the affected snapshots to the configured patches and record
/// the new hashes.
///
/// Runs after removals have been applied and pruned, so the unused-patch
/// guard and the rekey plan see the packages a full resolution would see
/// — a patch whose only referent was just removed falls back exactly
/// like the resolver raising `ERR_PNPM_UNUSED_PATCH` would.
///
/// `false` — a patched package reachable as a peer, an opaque peer
/// suffix, or a patch left unused while `allowUnusedPatches` is off —
/// leaves the caller on the full-resolution path.
pub(crate) fn apply_patched_update(
    candidate: &mut Lockfile,
    plan: &PatchedPlan,
    allow_unused_patches: bool,
) -> bool {
    if !allow_unused_patches {
        let Some(applied) = applied_patch_keys(candidate, &plan.groups) else {
            return false;
        };
        if all_patch_keys(&plan.groups).any(|key| !applied.contains(key)) {
            return false;
        }
    }
    let Some(rekeys) = plan_rekeys(candidate, &plan.groups) else {
        return false;
    };
    apply_rekeys(candidate, &rekeys);
    candidate.patched_dependencies = (!plan.current.is_empty()).then(|| plan.current.clone());
    true
}

/// Where every snapshot key moves to once `groups` is the configured
/// set of patches: the same `name@version` and peer suffix, carrying the
/// `(patch_hash=...)` segment the new configuration gives it.
///
/// `None` when the move cannot be made from lockfile data alone: a
/// rekeyed package that some snapshot reaches as a peer would rekey its
/// dependents too (their peer suffix embeds its depPath), and a peer
/// suffix that pnpm shortened into a hash cannot be inspected for that
/// at all.
fn plan_rekeys(lockfile: &Lockfile, groups: &PatchGroupRecord) -> Option<Rekeys> {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return Some(Rekeys::new());
    };
    let mut rekeys = Rekeys::new();
    for key in snapshots.keys() {
        let rendered = key.to_string();
        let suffix = index_of_dep_path_suffix(&rendered);
        let base = remove_suffix(&rendered);
        let peers = suffix.peers_index.map_or("", |index| &rendered[index..]);
        let (name, version) = pnpm_deps_restorer::parse_name_version_from_key(base);
        let patch = get_patch_info(Some(groups), &name, &version).ok()?;
        // The resolver matches patches against a package's plain semver
        // version, while this reads the version out of the key, where a
        // named registry (`name@registry:version`) or a git / tarball
        // reference occupies the same slot. The two only agree on plain
        // semver, so anything else is left to the resolver rather than
        // guessed at — as long as it needs no rekey at all.
        if key.suffix.version_semver().is_none() {
            // Matching cannot be reproduced here, so the question is only
            // whether it could matter: any configured patch naming this
            // package, or a patch hash already on the key, hands the
            // decision back to the resolver.
            if groups.contains_key(name.as_str()) || suffix.patch_hash_index.is_some() {
                return None;
            }
            continue;
        }
        let segment = match patch {
            Some(patch) => format!("(patch_hash={})", patch.hash),
            None => String::new(),
        };
        let moved = format!("{base}{segment}{peers}");
        if moved != rendered {
            rekeys.insert(key.clone(), moved.parse().ok()?);
        }
    }
    if rekeys.is_empty() {
        return Some(rekeys);
    }

    let moved_bases: Vec<String> =
        rekeys.keys().map(|key| remove_suffix(&key.to_string()).to_string()).collect();
    for key in snapshots.keys() {
        let rendered = key.to_string();
        let Some(index) = index_of_dep_path_suffix(&rendered).peers_index else {
            continue;
        };
        let peers = &rendered[index..];
        if peer_suffix_is_opaque(peers)
            || moved_bases.iter().any(|base| peers.contains(base.as_str()))
        {
            return None;
        }
    }
    Some(rekeys)
}

/// Whether `peers` is the short hash pnpm substitutes once the joined
/// peer segments exceed `peersSuffixMaxLength`, which hides the peers
/// this rewrite has to check for.
fn peer_suffix_is_opaque(peers: &str) -> bool {
    peers
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(")(")
        .any(|segment| !segment.contains('@'))
}

fn apply_rekeys(lockfile: &mut Lockfile, rekeys: &Rekeys) {
    if rekeys.is_empty() {
        return;
    }
    if let Some(snapshots) = lockfile.snapshots.take() {
        lockfile.snapshots = Some(
            snapshots
                .into_iter()
                .map(|(key, mut snapshot)| {
                    rewrite_snapshot_dependencies(&mut snapshot.dependencies, rekeys);
                    rewrite_snapshot_dependencies(&mut snapshot.optional_dependencies, rekeys);
                    (rekeys.get(&key).cloned().unwrap_or(key), snapshot)
                })
                .collect(),
        );
    }
    for importer in lockfile.importers.values_mut() {
        rewrite_importer_dependencies(importer, rekeys);
    }
}

fn rewrite_snapshot_dependencies(
    dependencies: &mut Option<HashMap<pnpm_lockfile::PkgName, SnapshotDepRef>>,
    rekeys: &Rekeys,
) {
    let Some(dependencies) = dependencies.as_mut() else {
        return;
    };
    for (alias, reference) in dependencies.iter_mut() {
        let Some(moved) = reference.resolve(alias).and_then(|target| rekeys.get(&target)).cloned()
        else {
            continue;
        };
        match reference {
            SnapshotDepRef::Plain(ver_peer) => *ver_peer = moved.suffix,
            SnapshotDepRef::Alias(key) => *key = moved,
            // `resolve` yields no key for a link, so it never matches a
            // rekeyed package.
            SnapshotDepRef::Link(_) => {}
        }
    }
}

fn rewrite_importer_dependencies(importer: &mut ProjectSnapshot, rekeys: &Rekeys) {
    for group in [
        importer.dependencies.as_mut(),
        importer.dev_dependencies.as_mut(),
        importer.optional_dependencies.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        rewrite_importer_group(group, rekeys);
    }
}

fn rewrite_importer_group(group: &mut ResolvedDependencyMap, rekeys: &Rekeys) {
    for (alias, spec) in group.iter_mut() {
        let target = match &spec.version {
            ImporterDepVersion::Regular(ver_peer) => {
                PackageKey::new(alias.clone(), ver_peer.clone())
            }
            ImporterDepVersion::Alias(key) => key.clone(),
            // The remaining shapes point outside `snapshots:`, so they
            // never name a rekeyed package.
            _ => continue,
        };
        let Some(moved) = rekeys.get(&target).cloned() else {
            continue;
        };
        match &mut spec.version {
            ImporterDepVersion::Regular(ver_peer) => *ver_peer = moved.suffix,
            ImporterDepVersion::Alias(key) => *key = moved,
            _ => {}
        }
    }
}

/// Whether every configured patch still has a package to apply to.
///
/// A patch with none left is `ERR_PNPM_UNUSED_PATCH`, which only a
/// resolution raises, so a rewrite that would produce one has to decline
/// and let the resolver report it. Any handler that drops an edge can
/// produce one, not only a changed patch configuration, so this runs over
/// the settled graph rather than inside [`apply_patched_update`].
///
/// `allowUnusedPatches` turns that error into a warning the resolution
/// emits, which no rewrite reproduces either; as elsewhere in this module,
/// a warning is not worth a full resolution.
pub(crate) fn every_configured_patch_is_applied(
    lockfile: &Lockfile,
    hashes: Option<&BTreeMap<String, String>>,
    allow_unused_patches: bool,
) -> bool {
    if allow_unused_patches {
        return true;
    }
    let Some(hashes) = hashes.filter(|hashes| !hashes.is_empty()) else {
        return true;
    };
    let Some(groups) = groups_from_hashes(hashes) else {
        return false;
    };
    let Some(applied) = applied_patch_keys(lockfile, &groups) else {
        return false;
    };
    !all_patch_keys(&groups).any(|key| !applied.contains(key))
}

/// The configured patches the committed lockfile leaves unused, as the
/// resolution's own `verify_patches` would report them.
///
/// Only non-empty with `allowUnusedPatches` on: with it off,
/// [`every_configured_patch_is_applied`] declines the rewrite instead, and
/// the resolution that takes over raises `ERR_PNPM_UNUSED_PATCH`.
pub(crate) fn unused_patches(
    lockfile: &Lockfile,
    hashes: Option<&BTreeMap<String, String>>,
) -> Option<pnpm_patching::UnusedPatches> {
    let hashes = hashes.filter(|hashes| !hashes.is_empty())?;
    let groups = groups_from_hashes(hashes)?;
    let applied = applied_patch_keys(lockfile, &groups)?;
    let applied = applied.into_iter().map(str::to_string).collect();
    pnpm_patching::verify_patches(&groups, &applied, true).ok().flatten()
}

/// Bucket `hashes` the way the resolver buckets configured patches.
///
/// The patch file path is left out: nothing here applies a patch, and
/// the hashes [`pnpm_config::Config::patched_dependency_hashes`]
/// already computed are the only payload the rewrite needs, so no patch
/// file is read twice.
///
/// `None` for a key whose version segment is neither a version nor a
/// range, leaving `ERR_PNPM_PATCH_NON_SEMVER_RANGE` to the resolver.
fn groups_from_hashes(hashes: &BTreeMap<String, String>) -> Option<PatchGroupRecord> {
    group_patched_dependencies(
        hashes.iter().map(|(key, hash)| {
            (key.clone(), PatchInput { hash: hash.clone(), patch_file_path: None })
        }),
    )
    .ok()
}

/// The patch keys in `patch_groups` that match a package the lockfile
/// records, matched the way the resolver matches them.
///
/// `None` when a locked package matches more than one configured range,
/// so the caller falls back and lets the resolver raise
/// `ERR_PNPM_PATCH_KEY_CONFLICT` instead of quietly picking a winner.
fn applied_patch_keys<'a>(
    lockfile: &Lockfile,
    patch_groups: &'a PatchGroupRecord,
) -> Option<BTreeSet<&'a str>> {
    let Some(snapshots) = lockfile.snapshots.as_ref() else {
        return Some(BTreeSet::new());
    };
    let mut applied = BTreeSet::new();
    for key in snapshots.keys() {
        // Keyed exactly as `resolve_snapshot_patches` keys the patches it
        // applies from a loaded lockfile, so this agrees with what the
        // materializer would do. The peer suffix carries any
        // `(patch_hash=...)` segment too, so stripping it leaves the
        // `name@version` the patch keys match on.
        let metadata_key = key.without_peer().to_string();
        let (name, version) = pnpm_deps_restorer::parse_name_version_from_key(&metadata_key);
        if let Some(info) = get_patch_info(Some(patch_groups), &name, &version).ok()? {
            applied.insert(info.key.as_str());
        }
    }
    Some(applied)
}

#[cfg(test)]
mod tests;
