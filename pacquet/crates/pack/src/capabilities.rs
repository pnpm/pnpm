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

use std::{io, path::Path};

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
/// **Not atomic** — the moral equivalent of `std::fs::write`. Matches
/// upstream, which streams straight into a `fs.createWriteStream`.
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
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }
}
