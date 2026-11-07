//! Taking a global-virtual-store slot for a build.

use std::path::Path;

use pnpm_lockfile::LockfileResolution;

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
#[derive(Clone, Copy)]
pub(super) struct SlotToBuild<'a> {
    pub(super) context: &'a BuildOneSnapshot<'a>,
    pub(super) snapshot_key: &'a PackageKey,
    pub(super) candidate: &'a BuildCandidate<'a>,
    pub(super) cache_key: Option<&'a str>,
}

pub(super) fn slot_to_build(
    input: SlotToBuild<'_>,
) -> Result<Option<(PathBuf, Option<crate::GlobalVirtualStoreSlot>)>, BuildModulesError> {
    let SlotToBuild {
        context, snapshot_key, candidate, ..
    } = input;
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
    let marker = pkg_dir.join(NEEDS_BUILD_MARKER);
    if built_by_an_earlier_install(&input, &marker)? {
        return Ok(None);
    }
    if let Some(global_slot) = &slot {
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
        if state.phase == crate::GlobalVirtualStoreSlotPhase::Building {
            return Ok(None);
        }
        global_slot
            .mark_build_started()
            .map_err(|source| BuildModulesError::WriteBuildMarker { path: marker, source })?;
    } else if is_started_build_marker(&marker)
        .map_err(|source| BuildModulesError::ReadBuildMarker { path: marker.clone(), source })?
    {
        return Ok(None);
    }
    Ok(Some((pkg_dir, slot)))
}

fn built_by_an_earlier_install(
    input: &SlotToBuild<'_>,
    marker: &Path,
) -> Result<bool, BuildModulesError> {
    let SlotToBuild {
        context,
        snapshot_key,
        candidate,
        cache_key,
    } = input;
    let from_an_immutable_source = context.graph.packages
        .and_then(|packages| packages.get(&candidate.metadata_key))
        .is_some_and(|metadata| !matches!(metadata.resolution, LockfileResolution::Directory(_)));
    if candidate.force_rebuild
        || !from_an_immutable_source
        || !context.directories.layout.enable_global_virtual_store()
        || cache_key.is_some_and(|key| context.cache.side_effects.has_empty_row(snapshot_key, key))
    {
        return Ok(false);
    }
    crate::build_modules::slots::built_slot_is_complete(
        marker.parent().expect("build marker has a package directory"),
    )
}
