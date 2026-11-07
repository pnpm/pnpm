//! Taking a global-virtual-store slot for a build.

use std::path::Path;

use pnpm_lockfile::LockfileResolution;

use super::{
    super::{BuildModulesError, NEEDS_BUILD_MARKER, PackageKey, PathBuf, is_started_build_marker},
    BuildCandidate, BuildOneSnapshot,
};

/// The package directory and locks held while applying its build. In a
/// global virtual store, the package and dependency slots are locked in
/// path order so bin refresh cannot invert their order with another build.
/// `None` when the snapshot has no directory, another install built its
/// slot while this one waited, or another install's build did not finish.
#[derive(Clone, Copy)]
pub(super) struct SlotToBuild<'a> {
    pub(super) context: &'a BuildOneSnapshot<'a>,
    pub(super) snapshot_key: &'a PackageKey,
    pub(super) candidate: &'a BuildCandidate<'a>,
    pub(super) cache_key: Option<&'a str>,
}

pub(super) struct LockedSlotToBuild {
    pub(super) package_dir: PathBuf,
    pub(super) slot: Option<crate::GlobalVirtualStoreSlot>,
    pub(super) dependency_slots: Vec<crate::GlobalVirtualStoreSlot>,
}

pub(super) fn slot_to_build(
    input: SlotToBuild<'_>,
) -> Result<Option<LockedSlotToBuild>, BuildModulesError> {
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
    if !(candidate.patch.is_some() || candidate.should_run_scripts) {
        return Ok(Some(LockedSlotToBuild {
            package_dir: pkg_dir,
            slot: None,
            dependency_slots: Vec::new(),
        }));
    }
    if context.directories.pkg_roots_by_key.is_some()
        || !context.directories.layout.enable_global_virtual_store()
    {
        return Ok(Some(LockedSlotToBuild {
            package_dir: pkg_dir,
            slot: None,
            dependency_slots: Vec::new(),
        }));
    }
    if context.directories.layout.hashed_slot_dir(snapshot_key).is_none() {
        if built_by_an_earlier_install(&input, &pkg_dir.join(NEEDS_BUILD_MARKER))? {
            return Ok(None);
        }
        if !mark_slot_build_started(
            None,
            &pkg_dir,
            snapshot_key,
            &pkg_dir.join(NEEDS_BUILD_MARKER),
        )? {
            return Ok(None);
        }
        return Ok(Some(LockedSlotToBuild {
            package_dir: pkg_dir,
            slot: None,
            dependency_slots: Vec::new(),
        }));
    }
    let mut slot_keys = vec![snapshot_key.clone()];
    if let Some(snapshot) = context.graph.snapshots.get(snapshot_key) {
        slot_keys.extend(
            snapshot.dependencies
                .iter()
                .flatten()
                .chain(snapshot.optional_dependencies.iter().flatten())
                .filter_map(|(alias, dependency)| dependency.resolve(alias))
                .filter(|key| context.directories.layout.hashed_slot_dir(key).is_some()),
        );
    }
    let mut slot_locks =
        crate::GlobalVirtualStoreSlot::acquire_ordered(context.directories.layout, slot_keys)
            .map_err(BuildModulesError::SlotLock)?;
    let package_slot_path = context.directories.layout.slot_dir(snapshot_key);
    let package_slot_index = slot_locks
        .iter()
        .position(|slot| slot.path() == package_slot_path)
        .expect("the package slot is included in its build lock set");
    let slot = slot_locks.swap_remove(package_slot_index);
    let marker = pkg_dir.join(NEEDS_BUILD_MARKER);
    if built_by_an_earlier_install(&input, &marker)? {
        return Ok(None);
    }
    if !mark_slot_build_started(Some(&slot), &pkg_dir, snapshot_key, &marker)? {
        return Ok(None);
    }
    Ok(Some(LockedSlotToBuild {
        package_dir: pkg_dir,
        slot: Some(slot),
        dependency_slots: slot_locks,
    }))
}

fn mark_slot_build_started(
    slot: Option<&crate::GlobalVirtualStoreSlot>,
    package_dir: &Path,
    snapshot_key: &PackageKey,
    marker: &Path,
) -> Result<bool, BuildModulesError> {
    let Some(slot) = slot else {
        return is_started_build_marker(marker)
            .map(|started| !started)
            .map_err(|source| BuildModulesError::ReadBuildMarker {
                path: marker.to_path_buf(),
                source,
            });
    };
    let state = slot
        .state(&package_dir.join("package.json"), crate::BuildRequirement::Required)
        .map_err(|source| BuildModulesError::ReadBuildMarker {
            path: marker.to_path_buf(),
            source,
        })?;
    if state.phase == crate::GlobalVirtualStoreSlotPhase::Populating {
        return Err(BuildModulesError::IncompleteGvsSlot {
            dep_path: snapshot_key.to_string(),
            path: slot.path().to_path_buf(),
        });
    }
    if state.phase == crate::GlobalVirtualStoreSlotPhase::Building {
        return Ok(false);
    }
    slot.mark_build_started()
        .map_err(|source| BuildModulesError::WriteBuildMarker {
            path: marker.to_path_buf(),
            source,
        })?;
    Ok(true)
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
