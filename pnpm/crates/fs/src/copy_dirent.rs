use std::{fs, io, path::Path};

/// Copy whatever occupies `src` to `dst` without following links: a
/// symlink is recreated pointing at the same target, a directory is
/// created and its contents copied recursively, and anything else is
/// copied byte for byte.
///
/// Recreating links instead of following them is what makes the copy
/// usable on trees that point into the store or the virtual store: a
/// `node_modules/` is mostly symlinks, and following one would turn a
/// link into a second copy of the package it names.
pub fn copy_dirent(src: &Path, dst: &Path) -> io::Result<()> {
    copy_entry(src, dst, fs::symlink_metadata(src)?.file_type())
}

/// Copy every entry of the directory `src` into the existing directory
/// `dst`, each with [`copy_dirent`]'s treatment.
pub fn copy_dir_contents(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        copy_entry(&entry.path(), &dst.join(entry.file_name()), entry.file_type()?)?;
    }
    Ok(())
}

fn copy_entry(src: &Path, dst: &Path, file_type: fs::FileType) -> io::Result<()> {
    if file_type.is_symlink() {
        return copy_symlink(src, dst, file_type);
    }
    if file_type.is_dir() {
        fs::create_dir_all(dst)?;
        return copy_dir_contents(src, dst);
    }
    fs::copy(src, dst).map(drop)
}

/// Recreate `src`'s link at `dst`. Windows types its links at creation
/// time, so a directory link has to be recreated with `symlink_dir`;
/// the target is not resolved either way, so a dangling link survives
/// the copy as a dangling link.
fn copy_symlink(src: &Path, dst: &Path, file_type: fs::FileType) -> io::Result<()> {
    let target = fs::read_link(src)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let target = crate::to_native_separators(&target);
        let dst = crate::to_native_separators(dst);
        if file_type.is_symlink_dir() {
            return std::os::windows::fs::symlink_dir(&target, &dst);
        }
        return std::os::windows::fs::symlink_file(&target, &dst);
    }
    #[cfg(unix)]
    {
        let _ = file_type;
        std::os::unix::fs::symlink(target, dst)
    }
}

#[cfg(test)]
mod tests;
