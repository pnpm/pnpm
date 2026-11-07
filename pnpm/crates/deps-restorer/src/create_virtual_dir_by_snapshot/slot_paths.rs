use super::CreateVirtualDirError;
use crate::{
    ImportIndexedDirOpts, NEEDS_BUILD_MARKER, VirtualStoreLayout,
    import_indexed_dir::marker_present,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
};
use pnpm_fs::{CasPaths, StoreFile};
use pnpm_lockfile::PackageKey;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The slot's directories, created.
pub(super) struct SlotPaths {
    pub(super) node_modules: PathBuf,
    pub(super) save_path: PathBuf,
}

impl SlotPaths {
    pub(super) fn create(
        layout: &VirtualStoreLayout,
        package_key: &PackageKey,
    ) -> Result<Self, CreateVirtualDirError> {
        if !layout.is_slot_contained(package_key) {
            return Err(CreateVirtualDirError::InvalidAlias(InvalidDependencyAliasError {
                modules: layout.package_store_dir().to_path_buf(),
                alias: package_key.to_string(),
            }));
        }
        let slot_dir = layout.slot_dir(package_key);
        let node_modules = slot_dir.join("node_modules");
        // Two direct `mkdir`s instead of one `create_dir_all` on the
        // deepest path: the recursive form probes bottom-up with a
        // failing `mkdir` per missing ancestor before creating them
        // top-down, which on the APFS-serialized metadata path costs a
        // large install ~3 extra syscalls per slot. The virtual-store
        // root exists (steady state) — only its absence falls back to
        // the recursive form.
        create_slot_dirs(&slot_dir, &node_modules)?;
        let save_path = safe_join_modules_dir(&node_modules, &package_key.name.to_string())
            .map_err(CreateVirtualDirError::InvalidAlias)?;
        Ok(Self { node_modules, save_path })
    }
}

/// Two direct `mkdir`s instead of one `create_dir_all` on the deepest path:
/// the recursive form probes bottom-up with a failing `mkdir` per missing
/// ancestor before creating them top-down, which on the APFS-serialized
/// metadata path costs a large install ~3 extra syscalls per slot. The
/// virtual-store root exists (steady state) — only its absence falls back to
/// the recursive form.
fn create_slot_dirs(
    slot_dir: &Path,
    virtual_node_modules_dir: &Path,
) -> Result<(), CreateVirtualDirError> {
    match fs::create_dir(slot_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(slot_dir)
                .map_err(|error| CreateVirtualDirError::CreateSlotDir {
                    dir: slot_dir.to_path_buf(),
                    error,
                })?;
        }
        Err(error) => {
            return Err(CreateVirtualDirError::CreateSlotDir {
                dir: slot_dir.to_path_buf(),
                error,
            });
        }
    }
    match fs::create_dir(virtual_node_modules_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(CreateVirtualDirError::CreateNodeModulesDir {
            dir: virtual_node_modules_dir.to_path_buf(),
            error,
        }),
    }
}

/// The CAS paths plus a `.pnpm-needs-build` marker, when the slot has to carry
/// one it does not already have.
///
/// A finished slot's completion marker is normally proof that its build
/// ran too, so the marker is left out. Two cases break that and have to
/// carry it anyway: a build this install interrupted, and a forced
/// re-import, which replaces the slot's files with the pristine base map
/// and so undoes whatever the build did to them. Without the marker
/// [`slot_carries_overlay`](crate::build_modules::slot_carries_overlay) would read the re-imported files as a
/// cache hit and skip the rebuild.
pub(super) fn cas_paths_with_build_marker(
    cas_paths: &CasPaths,
    save_path: &Path,
    needs_build_marker_source: Option<&Path>,
    forced: (bool, bool),
) -> Option<CasPaths> {
    let (interrupted_build, force_import) = forced;
    let source = needs_build_marker_source?;
    if !interrupted_build && !force_import && marker_present(save_path, cas_paths) {
        return None;
    }
    let mut paths = cas_paths.clone();
    paths.insert(NEEDS_BUILD_MARKER.to_string(), StoreFile::new(source.to_path_buf()));
    Some(paths)
}

/// Inputs to [`slot_import_opts`]'s force decision, named instead of
/// positional: four `bool`s in a tuple stopped being safe to read at a
/// glance once `source_exists` joined `source_is_mutable`.
#[derive(Clone, Copy)]
pub(super) struct SlotForceInputs {
    pub(super) interrupted_build: bool,
    pub(super) source_is_mutable: bool,
    /// See [`crate::SlotImportSource::source_exists`]. Ignored unless
    /// `source_is_mutable` is also set.
    pub(super) source_exists: bool,
    pub(super) force_import: bool,
}

pub(super) fn slot_import_opts<'a>(
    layout: &crate::VirtualStoreLayout,
    slot: SlotForceInputs,
    swap_lock: Option<&'a crate::ModulesSwapLock>,
) -> ImportIndexedDirOpts<'a> {
    let SlotForceInputs {
        interrupted_build,
        source_is_mutable,
        source_exists,
        force_import,
    } = slot;
    // Mutable sources can reuse a slot for different contents, so a complete
    // import may be stale.
    let safe_to_skip = layout.enable_global_virtual_store() && !source_is_mutable;
    // A mutable source that is currently missing must never be forced, even by
    // interrupted_build or force_import — see SlotImportSource::source_exists.
    if source_is_mutable && !source_exists {
        return ImportIndexedDirOpts { safe_to_skip, swap_lock, ..ImportIndexedDirOpts::default() };
    }
    if interrupted_build || source_is_mutable || force_import {
        return ImportIndexedDirOpts {
            force: true,
            reuse_hardlinks: source_is_mutable && !force_import && !interrupted_build,
            keep_modules_dir: true,
            safe_to_skip,
            symlink_targets: source_is_mutable.then_some(crate::SymlinkTargets::Imported),
            swap_lock,
        };
    }
    ImportIndexedDirOpts { safe_to_skip, swap_lock, ..ImportIndexedDirOpts::default() }
}
