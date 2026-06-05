use std::{fs, io, path::Path};
use walkdir::WalkDir;

#[must_use]
pub fn get_filenames_in_folder(path: &Path) -> Vec<String> {
    let mut files = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();

    files.sort();
    files
}

fn normalized_suffix(path: &Path, prefix: &Path) -> String {
    path.strip_prefix(prefix)
        .expect("strip prefix from path")
        .to_str()
        .expect("convert suffix to UTF-8")
        .replace('\\', "/")
}

#[must_use]
pub fn get_all_folders(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.expect("access entry"))
        .filter(|entry| entry.file_type().is_dir() || entry.file_type().is_symlink())
        .map(|entry| normalized_suffix(entry.path(), root))
        .filter(|suffix| !suffix.is_empty())
        .collect()
}

#[must_use]
pub fn get_all_files(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.expect("access entry"))
        .filter(|entry| !entry.file_type().is_dir())
        .map(|entry| normalized_suffix(entry.path(), root))
        .filter(|suffix| !suffix.is_empty())
        .collect()
}

// Helper function to check if a path is a symlink or junction
pub fn is_symlink_or_junction(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        // True symlinks land here when the process has the
        // `SeCreateSymbolicLinkPrivilege` (Developer Mode or admin),
        // which is the case on GitHub Actions Windows runners.
        // `junction::exists` only matches `IO_REPARSE_TAG_MOUNT_POINT`,
        // so combine it with `Path::is_symlink` to cover both reparse
        // tags `pacquet_fs::symlink_dir` can produce.
        Ok(junction::exists(path)? || path.is_symlink())
    }

    #[cfg(not(windows))]
    Ok(path.is_symlink())
}

/// Check if a file is executable.
#[cfg(unix)]
#[must_use]
pub fn is_path_executable(path: &Path) -> bool {
    use std::{fs::File, os::unix::prelude::*};
    let mode = File::open(path)
        .expect("open the file")
        .metadata()
        .expect("get metadata of the file")
        .mode();
    mode & 0b001_001_001 != 0
}
