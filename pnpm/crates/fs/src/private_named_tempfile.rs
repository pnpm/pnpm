use std::{io, path::Path};

/// Creates a named temporary file with mode 0600 from its first open on Unix and WASI.
#[inline]
pub fn private_named_tempfile_in(
    directory: impl AsRef<Path>,
) -> io::Result<tempfile::NamedTempFile> {
    #[cfg(not(target_os = "wasi"))]
    {
        tempfile::NamedTempFile::new_in(directory)
    }
    #[cfg(target_os = "wasi")]
    {
        tempfile::Builder::new().make_in(directory, |path| crate::create_new_with_mode(path, 0o600))
    }
}
