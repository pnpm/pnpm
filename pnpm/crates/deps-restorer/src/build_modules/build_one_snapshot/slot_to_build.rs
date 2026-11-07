//! Taking a global-virtual-store slot for a build.

use super::{
    super::{BuildModulesError, NEEDS_BUILD_MARKER, PackageKey, PathBuf, is_started_build_marker},
    BuildCandidate, BuildOneSnapshot,
};

/// The package directory to build in, with the lock that serializes a
/// build into an isolated global-virtual-store slot with every other
/// install's build of it. The slot is marked mid-build before it is
/// returned. `None` when there is nothing to build: the snapshot has no
/// directory, another install built the slot while this one waited for
/// its lock, or another install's build of it did not finish.
pub(super) fn slot_to_build(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Result<Option<(PathBuf, Option<crate::GlobalVirtualStoreSlot>)>, BuildModulesError> {
    let Some(pkg_dir) = context
        .pkg_roots()
        .canonical(snapshot_key)
        .filter(|dir| dir.exists())
    else {
        return Ok(None);
    };
    if context.directories.pkg_roots_by_key.is_some()
        || !(candidate.patch.is_some() || candidate.should_run_scripts)
    {
        return Ok(Some((pkg_dir, None)));
    }
    let slot = crate::GlobalVirtualStoreSlot::acquire(context.directories.layout, snapshot_key)
        .map_err(BuildModulesError::SlotLock)?;
    if let Some(global_slot) = &slot {
        let marker = pkg_dir.join(NEEDS_BUILD_MARKER);
        let state = global_slot
            .state(&pkg_dir.join("package.json"), true)
            .map_err(|source| BuildModulesError::ReadBuildMarker {
                path: marker.clone(),
                source,
            })?;
        if state.phase == crate::GlobalVirtualStoreSlotPhase::Populating {
            return Err(BuildModulesError::IncompleteGvsSlot {
                dep_path: snapshot_key.to_string(),
                path: global_slot.path().to_path_buf(),
            });
        }
        if state.phase == crate::GlobalVirtualStoreSlotPhase::Building
            || (state.phase == crate::GlobalVirtualStoreSlotPhase::Built
                && context.rebuild.is_none())
        {
            return Ok(None);
        }
        global_slot
            .mark_build_started()
            .map_err(|source| BuildModulesError::WriteBuildMarker { path: marker, source })?;
    } else {
        let marker = pkg_dir.join(NEEDS_BUILD_MARKER);
        if is_started_build_marker(&marker)
            .map_err(|source| BuildModulesError::ReadBuildMarker { path: marker.clone(), source })?
        {
            return Ok(None);
        }
    }
    Ok(Some((pkg_dir, slot)))
}
