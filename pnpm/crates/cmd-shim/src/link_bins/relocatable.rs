use crate::shim::is_relocatable_shim;
use pnpm_fs::is_subdir;
use std::{
    fs::{self, DirEntry},
    io,
    path::Path,
};

/// Whether every entry of `bin_dir` keeps working after `root` moves: a
/// relative symlink, or a shim whose target marker and `NODE_PATH` entries
/// name paths relative to it, resolving inside `root`. A missing `bin_dir`
/// passes. An unreadable one, or an entry of any other kind, fails.
#[must_use]
pub fn bin_dir_is_relocatable(bin_dir: &Path, root: &Path) -> bool {
    match fs::read_dir(bin_dir) {
        Ok(mut entries) => {
            entries.all(|entry| entry.is_ok_and(|entry| is_relocatable_bin(&entry, bin_dir, root)))
        }
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

fn is_relocatable_bin(entry: &DirEntry, bin_dir: &Path, root: &Path) -> bool {
    let path = entry.path();
    match entry.file_type() {
        Ok(file_type) if file_type.is_symlink() => fs::read_link(&path)
            .is_ok_and(|link| link.is_relative() && is_subdir(root, &bin_dir.join(link))),
        Ok(file_type) if file_type.is_file() => fs::read_to_string(&path)
            .is_ok_and(|content| is_relocatable_shim(&content, bin_dir, root)),
        _ => false,
    }
}
