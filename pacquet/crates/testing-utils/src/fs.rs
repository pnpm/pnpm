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

pub fn is_symlink_or_junction(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        // `pacquet_fs::symlink_dir` produces two reparse-tag shapes: a
        // real symlink (`IO_REPARSE_TAG_SYMLINK`, seen by
        // `Path::is_symlink` — true symlinks work on GitHub Actions
        // Windows runners, which grant `SeCreateSymbolicLinkPrivilege`)
        // or a junction (`IO_REPARSE_TAG_MOUNT_POINT`, seen by
        // `junction::exists`). Check the symlink case first so a plain
        // symlink never reaches `junction::exists`.
        if path.is_symlink() {
            return Ok(true);
        }
        // `junction::exists` reports a path that isn't a reparse point
        // at all (a plain directory) as `ERROR_NOT_A_REPARSE_POINT`
        // rather than `Ok(false)`; for "is this a symlink or junction?"
        // that is a plain "no", so map it back to `false`.
        const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;
        match junction::exists(path) {
            Ok(is_junction) => Ok(is_junction),
            Err(error) if error.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT) => Ok(false),
            Err(error) => Err(error),
        }
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
