//! Per-capability dependency-injection traits and the production
//! [`Host`] provider. Mirrors the pattern documented at
//! <https://github.com/pnpm/pacquet/pull/332#issuecomment-4345054524>:
//!
//! 1. One trait per capability.
//! 2. Functions bind only what they consume (compose bounds).
//! 3. No `&self` on capability methods.
//! 4. Production callers turbofish the real impl explicitly.
//!
//! Tests inject unit-struct fakes to exercise IO error paths that the
//! real filesystem can't reach portably (e.g. permission denied,
//! ENOSPC).

use pipe_trait::Pipe;
use std::{
    io,
    path::{Path, PathBuf},
};

/// Read up to `buf.len()` bytes of `path` starting at byte `offset`.
///
/// The returned `usize` is the number of bytes actually written into
/// `buf`. Like `std::io::Read::read`, an impl is allowed to return
/// fewer bytes than requested (a "short read") even when more data is
/// available, so callers that need a fully-filled buffer must loop.
/// [`crate::read_head_filled`] supplies that loop while staying
/// generic over this trait, so test fakes do not have to grow.
///
/// The trait makes no claim about how many syscalls a particular
/// impl will use — the production `Host` impl opens the file,
/// seeks to `offset` (if non-zero), and reads, which is more than
/// one. What it does promise is the semantic contract: read up to
/// `buf.len()` bytes starting at `offset` into `buf`.
///
/// Used by [`crate::search_script_runtime`] (via [`crate::read_head_filled`])
/// to detect the script runtime via the shebang at the head of a bin
/// file.
pub trait FsReadHead {
    fn read_head(path: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// Read the entire contents of a file into a `Vec<u8>`. Used to read
/// `package.json` files when collecting bin sources.
pub trait FsReadFile {
    fn read_file(path: &Path) -> io::Result<Vec<u8>>;
}

/// Read the entire contents of a file into a `String`. Used by
/// [`crate::link_bins_of_packages`] to short-circuit on warm reinstalls
/// where the existing shim already targets the same bin file.
pub trait FsReadToString {
    fn read_to_string(path: &Path) -> io::Result<String>;
}

/// List the entries of a directory.
///
/// Returns an `impl Iterator<Item = PathBuf>` rather than a
/// `Vec<PathBuf>`, so the production impl can stream entries straight
/// out of `fs::ReadDir` without materialising the whole list. The
/// associated-type-free shape also frees fakes from declaring an
/// `Iter` type per impl. Each fake just returns whatever concrete
/// iterator it wants.
///
/// We deliberately do not expose `fs::ReadDir` directly: its iterator
/// type is platform-specific and yields `io::Result<DirEntry>`,
/// which would force every fake to fabricate a `DirEntry` (and tie
/// the trait to libstd's filesystem types). Yielding plain
/// `PathBuf` keeps fakes trivial.
pub trait FsReadDir {
    fn read_dir(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>>;
}

/// Recursively walk `path` and yield every regular file found beneath
/// it (depth-first, no symlink follow). Used by
/// [`crate::get_bins_from_package_manifest`] to enumerate
/// `directories.bin` entries.
///
/// Returns an `impl Iterator<Item = PathBuf>` rather than a
/// `Vec<PathBuf>`, so the production walker streams entries straight
/// out of `walkdir` instead of materialising the whole list up front.
/// `directories.bin` trees are usually tiny in practice, but the
/// abstraction should not bake in an allocation the real
/// implementation does not need. Fakes return whatever concrete
/// iterator they want. [`std::iter::empty`] fits the unreachable-walk
/// case, and [`Vec::into_iter`] fits the case that feeds a fixed list
/// of paths.
///
/// `walkdir`'s builder exposes many knobs (`follow_links`, `min_depth`,
/// `max_depth`, `sort_by`, and so on); pacquet uses just one
/// (`follow_links = false`). Mirroring the full builder through the
/// trait would be over-engineering for the single call site, so the
/// trait keeps its surface dead-simple and the impl bakes the option
/// in. If a future caller needs different walk options, add a new
/// capability rather than parameterise this one.
pub trait FsWalkFiles {
    fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>>;
}

/// Create a directory and any missing ancestors. Used to prepare
/// `<modules_dir>/.bin` and per-slot `node_modules/.bin` directories.
pub trait FsCreateDirAll {
    fn create_dir_all(path: &Path) -> io::Result<()>;
}

/// Write `bytes` to `path`, replacing the file's contents if it
/// exists. Used to write the three shim flavors (`.sh`, `.cmd`,
/// `.ps1`).
///
/// **Not atomic.** This trait is the moral equivalent of
/// `std::fs::write`: it opens (or creates and truncates) the file,
/// writes `bytes`, and closes. No tempfile + rename guard, no
/// `fsync`. A SIGINT or crash mid-write can leave a truncated file
/// on disk. Number of syscalls is up to the impl — `std::fs::write`
/// itself is open/(truncate)/write/close, and a fake might loop.
/// If a future caller needs atomic write semantics, build it on top
/// of this trait by writing to a sibling tempfile and then
/// renaming. Hiding that algorithm inside the capability would
/// obscure what each callsite inherits; keeping the trait minimal
/// lets every callsite see exactly what guarantees it gets.
pub trait FsWrite {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()>;
}

/// Replace the permission bits at `path` with `0o755`. Used to chmod
/// the freshly written shim file so it is executable.
///
/// The method is always present so callers don't have to
/// `#[cfg(unix)]` every chmod call site. On Windows the production
/// impl is a no-op (Windows has no equivalent permission concept).
pub trait FsSetExecutable {
    fn set_executable(path: &Path) -> io::Result<()>;
}

/// Read the existing permission bits at `path`, OR in `0o111`, and
/// write them back. Used to add the executable bits to the underlying
/// target binary (mirrors pnpm's `fixBin`) without clobbering the
/// existing read/write bits the way [`FsSetExecutable`] would.
///
/// The method is always present for the same reason as
/// [`FsSetExecutable::set_executable`]; the production impl is a
/// no-op on Windows.
pub trait FsEnsureExecutableBits {
    fn ensure_executable_bits(path: &Path) -> io::Result<()>;
}

/// The production filesystem provider. Every method delegates straight
/// to `std::fs`.
pub struct Host;

impl FsReadHead for Host {
    fn read_head(path: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path)?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))?;
        }
        file.read(buf)
    }
}

impl FsReadFile for Host {
    fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

impl FsReadToString for Host {
    fn read_to_string(path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

impl FsReadDir for Host {
    fn read_dir(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
        // `flatten()` silently drops per-entry errors, matching the
        // `tinyglobby`-style ENOENT-on-subtree behaviour pacquet's
        // callers expect.
        std::fs::read_dir(path)?.flatten().map(|entry| entry.path()).pipe(Ok)
    }
}

impl FsWalkFiles for Host {
    fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
        // `flatten()` silently drops per-entry errors and matches
        // pnpm's `tinyglobby` ENOENT-on-subtree behaviour. The
        // top-level missing-dir case also flows through here as a
        // single dropped `Err`, so a missing `bin_dir` produces an
        // empty stream rather than an error.
        path.pipe(walkdir::WalkDir::new)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.path().to_path_buf())
            .pipe(Ok)
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

#[cfg(unix)]
impl FsSetExecutable for Host {
    fn set_executable(path: &Path) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }
}

#[cfg(not(unix))]
impl FsSetExecutable for Host {
    fn set_executable(_path: &Path) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(unix)]
impl FsEnsureExecutableBits for Host {
    fn ensure_executable_bits(path: &Path) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path)?;
        let mode = metadata.permissions().mode() | 0o111;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
}

#[cfg(not(unix))]
impl FsEnsureExecutableBits for Host {
    fn ensure_executable_bits(_path: &Path) -> io::Result<()> {
        Ok(())
    }
}
