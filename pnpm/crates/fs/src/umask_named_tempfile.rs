use std::{io, path::Path};

/// Creates a named temporary file with the mode a new file from `fs::write` gets:
/// 0666 narrowed by the umask on Unix and WASI.
///
/// For files other users may need to read, such as a `node_modules` built by root
/// in one Docker stage and used by another user in the next.
/// [`private_named_tempfile_in`](crate::private_named_tempfile_in) is the 0600 counterpart.
#[inline]
pub fn umask_named_tempfile_in(directory: impl AsRef<Path>) -> io::Result<tempfile::NamedTempFile> {
    named_tempfile_with_mode_in(directory, 0o666)
}

/// Creates a named temporary file with `mode` narrowed by the umask on Unix and WASI.
///
/// For a temp file that will replace an existing file: created with that file's mode,
/// its contents are never readable by more users than the file it replaces.
#[cfg_attr(
    not(target_os = "wasi"),
    expect(clippy::disallowed_methods, reason = "the helper the lint points to")
)]
pub fn named_tempfile_with_mode_in(
    directory: impl AsRef<Path>,
    mode: u32,
) -> io::Result<tempfile::NamedTempFile> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(mode))
            .tempfile_in(directory)
    }
    #[cfg(target_os = "wasi")]
    {
        tempfile::Builder::new().make_in(directory, |path| crate::create_new_with_mode(path, mode))
    }
    #[cfg(not(any(unix, target_os = "wasi")))]
    {
        let _ = mode;
        tempfile::NamedTempFile::new_in(directory)
    }
}
