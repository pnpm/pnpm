use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_patching::{
    PatchGroupRecord, PatchInput, all_patch_keys, get_patch_info, group_patched_dependencies,
};
use std::collections::{BTreeMap, BTreeSet};

/// Record a changed `patchedDependencies` without resolving the
/// dependency graph, for the changes that touch no package the lockfile
/// records.
///
/// A patch that matches a locked package contributes a
/// `(patch_hash=...)` segment to that package's key, so adding,
/// removing, or editing such a patch rekeys the graph and has to go
/// through the resolver. A patch key that matches nothing contributes
/// nothing, which makes the recorded map the only thing that changes.
///
/// `None` — nothing changed, a changed key matches a locked package, or
/// the new configuration leaves a patch unused while
/// `allowUnusedPatches` is off — leaves the caller on the
/// full-resolution path, which is where `ERR_PNPM_UNUSED_PATCH` is
/// raised.
///
/// A patch file that cannot be read or hashed also falls back, so the
/// resolver reports it rather than this path swallowing the error.
pub(crate) fn try_fast_update_patched_dependencies(
    lockfile: &Lockfile,
    config: &Config,
) -> Option<Lockfile> {
    let empty = BTreeMap::new();
    let hashes = config.patched_dependency_hashes().ok()?;
    let recorded = lockfile.patched_dependencies.as_ref().unwrap_or(&empty);
    let current = hashes.as_ref().unwrap_or(&empty);
    if recorded == current {
        return None;
    }

    // Every key whose presence or hash differs, taken from the recorded
    // map as well as the current one. A key the previous install applied
    // still owns a `(patch_hash=...)` segment in the recorded graph, so
    // dropping it rekeys that package just as adding it did.
    let affected = recorded
        .keys()
        .chain(current.keys())
        .filter(|key| recorded.get(*key) != current.get(*key))
        .cloned();
    if !applied_patch_keys(lockfile, &groups_from_keys(affected)?)?.is_empty() {
        return None;
    }

    if !config.allow_unused_patches {
        let current_groups = groups_from_keys(current.keys().cloned())?;
        let applied = applied_patch_keys(lockfile, &current_groups)?;
        if all_patch_keys(&current_groups).any(|key| !applied.contains(key)) {
            return None;
        }
    }

    let mut candidate = lockfile.clone();
    candidate.patched_dependencies = (!current.is_empty()).then(|| current.clone());
    Some(candidate)
}

/// Bucket `keys` the way the resolver buckets configured patches.
///
/// Only the key decides what a patch matches, so this deliberately
/// leaves the payload empty rather than re-reading the patch files that
/// [`Config::patched_dependency_hashes`] has already hashed.
///
/// `None` for a key whose version segment is neither a version nor a
/// range, leaving `ERR_PNPM_PATCH_NON_SEMVER_RANGE` to the resolver.
fn groups_from_keys(keys: impl IntoIterator<Item = String>) -> Option<PatchGroupRecord> {
    group_patched_dependencies(
        keys.into_iter()
            .map(|key| (key, PatchInput { hash: String::new(), patch_file_path: None })),
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
        let (name, version) = pacquet_deps_restorer::parse_name_version_from_key(&metadata_key);
        if let Some(info) = get_patch_info(Some(patch_groups), &name, &version).ok()? {
            applied.insert(info.key.as_str());
        }
    }
    Some(applied)
}

#[cfg(test)]
mod tests;
