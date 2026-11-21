use std::{
    fs,
    path::Path,
};

/// Whether `path`, whose [`fs::symlink_metadata`] is `metadata`, is a regular
/// file that another directory entry also names.
///
/// A file whose link count cannot be read counts as hard-linked, so a caller
/// that detaches shared files copies it rather than writing through a link.
#[cfg(unix)]
#[must_use]
pub fn is_hardlinked(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    metadata.is_file() && metadata.nlink() > 1
}

/// See the Unix variant.
#[cfg(windows)]
#[must_use]
pub fn is_hardlinked(path: &Path, metadata: &fs::Metadata) -> bool {
    metadata.is_file()
        && pnpm_ffi_fs::file_information(path).map_or(true, |info| info.hard_links > 1)
}

/// See the Unix variant.
#[cfg(target_os = "wasi")]
#[must_use]
pub fn is_hardlinked(path: &Path, metadata: &fs::Metadata) -> bool {
    metadata.is_file()
        && crate::open_file_without_following(path)
            .and_then(|file| crate::file_link_count(&file))
            .map_or(true, |count| count > 1)
}

/// See the Unix variant. This platform reports no link count.
#[cfg(not(any(unix, windows, target_os = "wasi")))]
#[must_use]
pub fn is_hardlinked(_path: &Path, metadata: &fs::Metadata) -> bool {
    metadata.is_file()
}
