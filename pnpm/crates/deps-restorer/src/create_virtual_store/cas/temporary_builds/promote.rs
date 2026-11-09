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
    // `--force` builds a placed package again, and its build replaces it.
    let when_placed =
        if installer.ctx.config.force { WhenPlaced::Replace } else { WhenPlaced::Keep };
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
        if when_placed.keeps_placed() && placed(layout, key)? {
            continue;
        }
        promote(temporary, layout, key, (&target, when_placed))
            .map_err(|source| CreateVirtualStoreError::LoadedBuildIo {
                path: target.clone(),
                source,
            })?;
    }
    Ok(retained)
}

/// What promoting a package does to a package already in the slot.
#[derive(Clone, Copy)]
enum WhenPlaced {
    /// Another install placed the package first, and it stays.
    Keep,
    Replace,
}

impl WhenPlaced {
    fn keeps_placed(self) -> bool {
        match self {
            WhenPlaced::Keep => true,
            WhenPlaced::Replace => false,
        }
    }
}

/// Whether the slot already holds the package. A slot's package directory
/// is final once its `package.json` exists, so it is never replaced.
fn placed(layout: &VirtualStoreLayout, key: &PackageKey) -> Result<bool, CreateVirtualStoreError> {
    let package_dir = crate::build_modules::slots::virtual_store_dir_for_key(layout, key);
    crate::build_modules::slots::package_is_placed(&package_dir)
        .map_err(|source| CreateVirtualStoreError::LoadedBuildIo { path: package_dir, source })
}

/// Move the slot `temporary` built for `key` into the shared store. A slot
/// that does not exist yet is renamed into place whole. Otherwise the
/// dependency links the slot lacks are moved in, and the package directory
/// is placed by the rule every placement follows: a package directory
/// without `package.json`, an import that did not finish, is replaced.
fn promote(
    temporary: &VirtualStoreLayout,
    layout: &VirtualStoreLayout,
    key: &PackageKey,
    (target, when_placed): (&Path, WhenPlaced),
) -> io::Result<()> {
    let source = temporary.slot_dir(key).join("node_modules");
    rewrite_links(&source, temporary.package_store_dir(), layout.package_store_dir())?;
    let parent = target.parent().expect("slot node_modules has a parent");
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".pnpm-loaded-build-").tempdir_in(parent)?;
    let staged = staging.path().join("node_modules");
    pnpm_fs::rename_even_across_devices::<pnpm_fs::Host>(&source, &staged)?;
    if !target.exists() && pnpm_fs::rename_with_retry(&staged, target).is_ok() {
        return Ok(());
    }
    let package_dir = crate::build_modules::slots::virtual_store_dir_for_key(layout, key);
    let relative = package_dir.strip_prefix(target).expect("the package sits in the slot");
    merge_missing_entries(&staged, target, relative)?;
    place_package(
        (&staged.join(relative), &package_dir),
        &staging.path().join("previous"),
        when_placed,
    )
}

/// Move every entry of `staged`, scope directories entered, that `target`
/// lacks into it: the dependency links and bins of a slot an interrupted
/// install left partly linked. `package` is left for [`place_package`].
fn merge_missing_entries(staged: &Path, target: &Path, package: &Path) -> io::Result<()> {
    for name in slot_entries(staged)?
        .iter()
        .filter(|name| name.as_path() != package)
    {
        let destination = target.join(name);
        if fs::symlink_metadata(&destination).is_ok() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(error) = pnpm_fs::rename_with_retry(&staged.join(name), &destination) {
            // Another install may have moved the same link in meanwhile.
            if fs::symlink_metadata(&destination).is_err() {
                return Err(error);
            }
        }
    }
    Ok(())
}

/// The entries of the `node_modules` at `dir`, relative to it, with the
/// entries of a scope directory in place of the scope.
fn slot_entries(dir: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let relative = std::path::PathBuf::from(entry?.file_name());
        let is_scope = relative.to_string_lossy().starts_with('@')
            && fs::symlink_metadata(dir.join(&relative))?.is_dir();
        if !is_scope {
            names.push(relative);
            continue;
        }
        for child in fs::read_dir(dir.join(&relative))? {
            names.push(relative.join(child?.file_name()));
        }
    }
    Ok(names)
}

fn place_package(
    (staged, target): (&Path, &Path),
    aside: &Path,
    when_placed: WhenPlaced,
) -> io::Result<()> {
    const ATTEMPTS: usize = 3;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    for attempt in 1..=ATTEMPTS {
        let Err(error) = rename_into_place(staged, target) else { return Ok(()) };
        if when_placed.keeps_placed() && crate::build_modules::slots::package_is_placed(target)? {
            return Ok(());
        }
        if !target.exists() || attempt == ATTEMPTS {
            return Err(error);
        }
        move_aside(target, aside)?;
    }
    unreachable!("the last attempt returns")
}

/// A target that exists is a lost race or a replacement, which a rename
/// retried for transient Windows file locks would only delay.
fn rename_into_place(staged: &Path, target: &Path) -> io::Result<()> {
    if target.exists() {
        fs::rename(staged, target)
    } else {
        pnpm_fs::rename_with_retry(staged, target)
    }
}

/// Move `target` out of the way and remove it. A directory another
/// install moved first is already gone.
fn move_aside(target: &Path, aside: &Path) -> io::Result<()> {
    match pnpm_fs::rename_with_retry(target, aside) {
        Ok(()) => fs::remove_dir_all(aside),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rewrite_links(directory: &Path, source: &Path, target: &Path) -> io::Result<()> {
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
        let entry = entry.map_err(io::Error::from)?;
        if !entry.file_type().is_symlink() {
            continue;
        }
        let destination = pnpm_fs::read_symlink_dir(entry.path())?;
        // A junction's target reads back in verbatim `\\?\` form on Windows.
        let Ok(relative) = dunce::simplified(&destination).strip_prefix(dunce::simplified(source))
        else {
            continue;
        };
        let replacement = target.join(relative);
        #[cfg(windows)]
        let is_directory = {
            use std::os::windows::fs::FileTypeExt;
            entry.file_type().is_symlink_dir()
        };
        #[cfg(not(windows))]
        let is_directory = false;
        pnpm_fs::remove_dirent(entry.path())?;
        if is_directory {
            pnpm_fs::symlink_dir_with_contents(&replacement, &replacement, entry.path())?;
        } else {
            pnpm_fs::create_symlink(&replacement, entry.path(), false)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
