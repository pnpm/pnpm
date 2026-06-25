//! Per-capability dependency-injection traits and the production
//! [`Host`] provider for the tarball-materialization filesystem
//! effects. Mirrors the seam documented at
//! <https://github.com/pnpm/pacquet/pull/332#issuecomment-4345054524>
//! and used by `pacquet-cmd-shim`:
//!
//! 1. One trait per capability.
//! 2. Functions bind only what they consume (compose bounds on one `Sys`).
//! 3. No `&self` on capability methods.
//! 4. Production callers turbofish [`Host`] explicitly.
//!
//! The seam covers only the final "write the tarball to disk" phase of
//! [`crate::api`] — reading each packed file's bytes, measuring its
//! size, creating the destination directory, and writing the archive.
//! Manifest reading, the packlist walk, and bin resolution stay on real
//! `std::fs` because real fixtures (a `tempfile::TempDir`) reach every
//! branch they have; the write phase is where a portable
//! `PermissionDenied` / `ENOSPC` test needs a fake.

use std::{
    io::{self, Write},
    path::Path,
};

/// Read the entire contents of a file into a `Vec<u8>`. Supplies the
/// bytes for each non-manifest tar entry.
pub trait FsReadFile {
    fn read_file(path: &Path) -> io::Result<Vec<u8>>;
}

/// Return a file's size in bytes (`std::fs::metadata(path)?.len()`),
/// used to accumulate the reported uncompressed tarball size.
pub trait FsFileLen {
    fn file_len(path: &Path) -> io::Result<u64>;
}

/// Create a directory and any missing ancestors, for the `--out` /
/// `--pack-destination` target directory.
pub trait FsCreateDirAll {
    fn create_dir_all(path: &Path) -> io::Result<()>;
}

/// Write `bytes` to `path`, replacing existing contents. Persists the
/// compressed archive.
///
/// The production [`Host`] writes atomically (sibling temp file + rename),
/// so the rename replaces a symlink at `path` instead of following it and
/// a crash never leaves a truncated `.tgz` behind. See the `Host` impl.
pub trait FsWrite {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// The production filesystem provider. Every method delegates straight
/// to `std::fs`.
pub struct Host;

impl FsReadFile for Host {
    fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

impl FsFileLen for Host {
    fn file_len(path: &Path) -> io::Result<u64> {
        std::fs::metadata(path).map(|metadata| metadata.len())
    }
}

impl FsCreateDirAll for Host {
    fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }
}

impl FsWrite for Host {
    /// Write the tarball atomically: a sibling temp file is written and
    /// fsynced, then renamed over `path`. The rename replaces a symlink
    /// sitting at the output path rather than following it — so a
    /// repo-controlled symlink can't redirect the write to clobber an
    /// arbitrary file — and a crash never leaves a partial `.tgz` behind.
    /// Mirrors the `write-file-atomic` pattern `pacquet-package-manifest`
    /// uses for `package.json`.
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_all()?;
        // A `NamedTempFile` is created 0o600. Match what a plain
        // `fs::write` would leave: preserve an existing tarball's mode when
        // overwriting, otherwise widen to 0o644 so the archive isn't
        // owner-only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .map_or(0o644, |metadata| metadata.permissions().mode() & 0o777);
            tmp.as_file().set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        tmp.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}
