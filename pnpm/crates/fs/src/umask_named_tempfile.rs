use std::{io, path::Path};

/// Creates a named temporary file with the mode a new file from `fs::write` gets:
/// 0666 narrowed by the umask on Unix and WASI.
///
/// For files other users may need to read, such as a `node_modules` built by root
/// in one Docker stage and used by another user in the next.
/// [`private_named_tempfile_in`](crate::private_named_tempfile_in) is the 0600 counterpart.
#[inline]
pub fn umask_named_tempfile_in(directory: impl AsRef<Path>) -> io::Result<tempfile::NamedTempFile> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o666))
            .tempfile_in(directory)
    }
    #[cfg(target_os = "wasi")]
    {
        tempfile::Builder::new().make_in(directory, |path| crate::create_new_with_mode(path, 0o666))
    }
    #[cfg(not(any(unix, target_os = "wasi")))]
    {
        tempfile::NamedTempFile::new_in(directory)
    }
}
