use super::{CreateVirtualStore, CreateVirtualStoreError, Snapshots};
use crate::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;
use std::{collections::HashSet, fs, io, path::Path};

pub(super) fn retain(
    installer: &CreateVirtualStore<'_>,
    temporary: &VirtualStoreLayout,
    candidates: &Snapshots,
    selected: &Snapshots,
) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
    let layout = installer.ctx.linker.layout;
    let retained: HashSet<_> = candidates
        .keys()
        .filter(|key| selected.contains_key(*key))
        .cloned()
        .collect();
    let mut moved = HashSet::new();
    for key in &retained {
        let target = layout.slot_dir(key).join("node_modules");
        if !moved.insert(target.clone()) {
            continue;
        }
        let _slot_lock = crate::gvs_slot_lock::lock_global_virtual_store_slot(layout, key)
            .ok_or_else(|| CreateVirtualStoreError::LoadedBuildIo {
                path: target.clone(),
                source: io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Cannot acquire the package build lock",
                ),
            })?;
        promote(temporary, layout, key, &target)
            .map_err(|source| CreateVirtualStoreError::LoadedBuildIo {
                path: target.clone(),
                source,
            })?;
    }
    Ok(retained)
}

fn promote(
    temporary: &VirtualStoreLayout,
    layout: &VirtualStoreLayout,
    key: &PackageKey,
    target: &Path,
) -> io::Result<()> {
    let source = temporary.slot_dir(key).join("node_modules");
    rewrite_links(&source, temporary.package_store_dir(), layout.package_store_dir())?;
    let parent = target.parent().expect("slot node_modules has a parent");
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".pnpm-loaded-build-").tempdir_in(parent)?;
    let staged = staging.path().join("node_modules");
    pnpm_fs::rename_even_across_devices::<pnpm_fs::Host>(&source, &staged)?;
    let backup = staging.path().join("previous");
    let replaced = match pnpm_fs::rename_with_retry(target, &backup) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    if let Err(error) = pnpm_fs::rename_with_retry(&staged, target) {
        if replaced {
            pnpm_fs::rename_with_retry(&backup, target)?;
        }
        return Err(error);
    }
    Ok(())
}

fn rewrite_links(directory: &Path, source: &Path, target: &Path) -> io::Result<()> {
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
        let entry = entry.map_err(io::Error::from)?;
        if !entry.file_type().is_symlink() {
            continue;
        }
        let destination = fs::read_link(entry.path())?;
        let Ok(relative) = destination.strip_prefix(source) else { continue };
        let replacement = target.join(relative);
        #[cfg(windows)]
        let is_directory = {
            use std::os::windows::fs::FileTypeExt;
            entry.file_type().is_symlink_dir()
        };
        #[cfg(not(windows))]
        let is_directory = false;
        pnpm_fs::remove_dirent(entry.path())?;
        pnpm_fs::create_symlink(&replacement, entry.path(), is_directory)?;
    }
    Ok(())
}
