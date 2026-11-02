use crate::shim::is_relocatable_shim;
use pnpm_fs::{is_subdir, realpath_missing};
use std::{
    fs::{self, DirEntry},
    io::{self, Read},
    path::Path,
};

/// A generated shim stays far below this, so a larger `.bin` entry is not one
/// and is refused without being held in memory.
const MAX_SHIM_BYTES: u64 = 64 * 1024;

/// Missing bin directories are valid; unreadable entries and paths resolving
/// outside `root` are not. Only relative symlinks and recognized shims qualify.
#[must_use]
pub fn bin_dir_is_relocatable(bin_dir: &Path, root: &Path) -> bool {
    let (Ok(root), Ok(bin_dir)) = (realpath_missing(root), realpath_missing(bin_dir)) else {
        return false;
    };
    if !is_subdir(&root, &bin_dir) {
        return false;
    }
    let bins_are_relocatable = match fs::read_dir(&bin_dir) {
        Ok(mut entries) => entries.all(|entry| {
            entry.is_ok_and(|entry| is_relocatable_bin(&entry, &bin_dir, &root))
        }),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    };
    bins_are_relocatable && alias_dir_is_relocatable(&bin_dir, &root)
}

fn alias_dir_is_relocatable(bin_dir: &Path, root: &Path) -> bool {
    if !cfg!(unix) {
        return true;
    }
    let Some(parent) = bin_dir.parent() else {
        return false;
    };
    let alias_dir = parent.join(".bin-symlinks");
    match fs::symlink_metadata(&alias_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => false,
        Ok(_) => alias_entries_are_relocatable(&alias_dir, root),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

fn alias_entries_are_relocatable(alias_dir: &Path, root: &Path) -> bool {
    match fs::read_dir(alias_dir) {
        Ok(mut entries) => {
            entries.all(|entry| entry.is_ok_and(|entry| is_alias_entry(&entry, alias_dir, root)))
        }
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

fn is_alias_entry(entry: &DirEntry, alias_dir: &Path, root: &Path) -> bool {
    entry
        .file_type()
        .is_ok_and(|file_type| {
            file_type.is_symlink() && is_relocatable_alias(entry, alias_dir, root)
        })
}

fn is_relocatable_alias(entry: &DirEntry, alias_dir: &Path, root: &Path) -> bool {
    fs::read_link(entry.path())
        .is_ok_and(|link| {
            link.is_relative()
                && realpath_missing(&alias_dir.join(link))
                    .is_ok_and(|target| is_subdir(root, &target))
        })
}

fn is_relocatable_bin(entry: &DirEntry, bin_dir: &Path, root: &Path) -> bool {
    let path = entry.path();
    match entry.file_type() {
        Ok(file_type) if file_type.is_symlink() => fs::read_link(&path)
            .is_ok_and(|link| {
                link.is_relative()
                    && realpath_missing(&bin_dir.join(link))
                        .is_ok_and(|target| is_subdir(root, &target))
            }),
        Ok(file_type) if file_type.is_file() => {
            read_shim(&path).is_some_and(|content| is_relocatable_shim(&content, bin_dir, root))
        }
        _ => false,
    }
}

/// The text of a file of at most [`MAX_SHIM_BYTES`], or `None` for a longer
/// one and for anything that is not readable text.
fn read_shim(path: &Path) -> Option<String> {
    let mut content = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_SHIM_BYTES + 1)
        .read_to_string(&mut content)
        .ok()?;
    (content.len() as u64 <= MAX_SHIM_BYTES).then_some(content)
}
