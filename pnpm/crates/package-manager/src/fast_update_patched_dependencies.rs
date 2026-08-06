use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_patching::{PatchGroupRecord, all_patch_keys, get_patch_info};
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

    // Grouping re-reads the patch files, so it waits until the cheap
    // map comparison above has proven there is drift to absorb.
    let patch_groups = config.resolved_patched_dependencies().ok()?;
    let applied = applied_patch_keys(lockfile, patch_groups.as_ref())?;
    let mut affected = recorded
        .iter()
        .filter(|(key, hash)| current.get(*key) != Some(hash))
        .chain(current.iter().filter(|(key, hash)| recorded.get(*key) != Some(hash)))
        .map(|(key, _)| key.as_str());
    if affected.any(|key| applied.contains(key)) {
        return None;
    }
    if !config.allow_unused_patches
        && patch_groups
            .as_ref()
            .into_iter()
            .flat_map(all_patch_keys)
            .any(|key| !applied.contains(key))
    {
        return None;
    }

    let mut candidate = lockfile.clone();
    candidate.patched_dependencies = (!current.is_empty()).then(|| current.clone());
    Some(candidate)
}

/// The configured patch keys that match a package the lockfile records,
/// matched the way the resolver matches them.
///
/// `None` when a locked package matches more than one configured range,
/// so the caller falls back and lets the resolver raise
/// `ERR_PNPM_PATCH_KEY_CONFLICT` instead of quietly picking a winner.
fn applied_patch_keys<'a>(
    lockfile: &Lockfile,
    patch_groups: Option<&'a PatchGroupRecord>,
) -> Option<BTreeSet<&'a str>> {
    let (Some(groups), Some(snapshots)) = (patch_groups, lockfile.snapshots.as_ref()) else {
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
        if let Some(info) = get_patch_info(Some(groups), &name, &version).ok()? {
            applied.insert(info.key.as_str());
        }
    }
    Some(applied)
}

#[cfg(test)]
mod tests;
