#[cfg(unix)]
use super::is_shim_pointing_at;
#[cfg(windows)]
use super::shim_writer::with_extension_appended;
use super::{FsEnsureExecutableBits, FsReadToString, LinkBinsError, Path, io, remove_stale_bin};

/// Make the underlying script executable: apply a minimum mode of
/// 0o755 without rewriting CRLF shebangs. Targets shipped by npm
/// already use LF in practice, so a chmod alone suffices.
pub(super) fn ensure_target_executable<Sys>(target_path: &Path) -> Result<(), LinkBinsError>
where
    Sys: FsEnsureExecutableBits,
{
    chmod_tolerating_removal(target_path, Sys::ensure_executable_bits)
}

/// Apply `chmod` to `path`, treating a path that has vanished as success.
///
/// Nothing serializes bin linking across processes: independent installs
/// sharing a global virtual store materialize the same shim, and an
/// unrelated process can remove a package between extraction and bin
/// linking. Whoever unlinked the path writes an equivalent one and chmods
/// it in turn, so `NotFound` means another writer finished the job rather
/// than that this one failed. Every other error still surfaces.
pub(super) fn chmod_tolerating_removal(
    path: &Path,
    chmod: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), LinkBinsError> {
    match chmod(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LinkBinsError::Chmod { path: path.to_path_buf(), error }),
    }
}

/// The `node_modules` directories relevant to a bin package in the
/// virtual-store layout — pnpm's `getBinNodePaths`. For a package at
/// `.pnpm/pkg@ver/node_modules/pkg` this returns the package's own
/// `node_modules` (bundled deps) followed by the slot's
/// `node_modules` (sibling deps), so tools that resolve from CWD
/// (`import-local` in jest, eslint, ...) find the correct versions.
/// `dir` must already be symlink-free — [`shim_node_path`](super::shim_node_path) passes the
/// caller-resolved location or a canonicalized fallback.
pub(super) fn bin_node_paths(dir: &Path) -> Vec<String> {
    let Some(node_modules_dir) = dir.ancestors().find(|ancestor| {
        ancestor.file_name().is_some_and(|name| name == "node_modules")
            && ancestor
                .parent()
                .and_then(Path::file_name)
                .is_none_or(|parent_name| parent_name != "node_modules")
    }) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    if let Ok(rel) = dir.strip_prefix(node_modules_dir)
        && let Some(first) = rel.components().next()
    {
        let first_name = first.as_os_str().to_string_lossy();
        let pkg_dir = if first_name.starts_with('@') {
            match rel.components().nth(1) {
                Some(second) => node_modules_dir.join(first).join(second.as_os_str()),
                None => node_modules_dir.join(first),
            }
        } else {
            node_modules_dir.join(first)
        };
        result.push(pkg_dir.join("node_modules").to_string_lossy().into_owned());
    }
    result.push(node_modules_dir.to_string_lossy().into_owned());
    result
}

/// Whether `shim_path`'s file name is exactly `node` — the trigger for the
/// node-runtime short-circuit in [`write_shim`](super::shim_writer::write_shim). Lifted out so the check
/// is unit-testable and the call site reads as a predicate.
pub(super) fn is_node_bin_name(shim_path: &Path) -> bool {
    matches!(shim_path.file_name().and_then(|s| s.to_str()), Some("node"))
}

/// Link the node runtime binary `target_path` into the bin slot
/// `shim_path` directly, without a cmd-shim wrapper. Returns `Ok(true)`
/// when the special case took effect (the caller must skip the regular
/// shim-writing path) and `Ok(false)` when it didn't apply and the
/// caller should fall through (Windows non-`.exe` source).
///
/// Two halves, by platform:
///
/// - **Unix** symlinks `shim_path` → absolute `target_path`. The
///   existing dirent (if any) is removed first because `fs::symlink`
///   rejects with `AlreadyExists` and we don't want to silently leave
///   a stale shim in place.
/// - **Windows** hardlinks `target_path` to `<shim_path>.exe`, falling
///   back to `fs::copy` on hardlink failure (cross-device, ACL deny,
///   ...). The source must end in `.exe`; otherwise the caller falls
///   through to the cmd-shim path.
///
/// `remove_file` rather than `Sys::write`-style truncation is
/// load-bearing on both platforms: if `shim_path` is currently a
/// regular file hardlinked to the source binary, truncating through
/// the hardlink would corrupt the binary itself. Removing the dirent
/// leaves the hardlinked content intact.
#[cfg(unix)]
pub(super) fn link_node_bin(target_path: &Path, shim_path: &Path) -> Result<bool, LinkBinsError> {
    use std::os::unix::fs::symlink;
    remove_stale_bin(shim_path)?;
    symlink(target_path, shim_path).map_err(|error| LinkBinsError::LinkNodeBin {
        src: target_path.to_path_buf(),
        dst: shim_path.to_path_buf(),
        error,
    })?;
    Ok(true)
}

#[cfg(windows)]
pub(super) fn link_node_bin(target_path: &Path, shim_path: &Path) -> Result<bool, LinkBinsError> {
    use std::fs;
    let is_exe = target_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return Ok(false);
    }
    let exe_path = with_extension_appended(shim_path, "exe");
    // Skip the remove + relink churn on warm installs when `node.exe`
    // already refers to the source binary.
    if is_same_file(&exe_path, target_path) {
        return Ok(true);
    }
    remove_stale_bin(&exe_path)?;
    if fs::hard_link(target_path, &exe_path).is_err() {
        fs::copy(target_path, &exe_path).map_err(|error| LinkBinsError::LinkNodeBin {
            src: target_path.to_path_buf(),
            dst: exe_path,
            error,
        })?;
    }
    Ok(true)
}

/// pnpm's `preferSymlinkedExecutables` bin materialization: a relative
/// symlink from the `.bin` entry to the target file, matching pnpm's
/// `symlink-dir` call (relative so a moved project keeps working; the
/// node runtime handled by [`link_node_bin`] keeps its absolute link,
/// also like pnpm). The target file — not the link — gets its
/// executable bits raised, and a dangling target is tolerated: the
/// symlink is created anyway with a warning, pnpm's
/// warn-and-continue.
///
/// Returns `Ok(true)` when the symlink path handled the bin, `Ok(false)`
/// when the caller must fall through to the shim path (Windows, where
/// the setting is inert — pnpm gates on `!isWindows()` the same way).
#[cfg(unix)]
pub(super) fn link_symlinked_executable<Sys>(
    target_path: &Path,
    shim_path: &Path,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadToString + FsEnsureExecutableBits,
{
    use std::os::unix::fs::symlink;
    // pnpm's warm-install short-circuit also accepts an existing shim
    // that points at the target, so enabling the setting rewrites no
    // valid shims — only bins that are missing or wrong get the
    // symlink form. (Symlinks already pointing at the target were
    // accepted before this branch was reached.)
    if matches!(
        Sys::read_to_string(shim_path),
        Ok(existing) if is_shim_pointing_at(&existing, target_path),
    ) {
        ensure_target_executable::<Sys>(target_path)?;
        return Ok(true);
    }
    let link_target = shim_path.parent().map_or_else(
        || target_path.to_path_buf(),
        |bins_dir| pnpm_fs::relative_path(bins_dir, target_path),
    );
    remove_stale_bin(shim_path)?;
    symlink(&link_target, shim_path).map_err(|error| LinkBinsError::SymlinkBin {
        src: target_path.to_path_buf(),
        dst: shim_path.to_path_buf(),
        error,
    })?;
    match Sys::ensure_executable_bits(target_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // pnpm's `Failed to create bin at ...` globalWarn: the
            // symlink dangles until a later step materializes the
            // target, which is worth telling the user about but not
            // worth failing the install over.
            let shim_path = shim_path.display();
            let target_path = target_path.display();
            tracing::warn!(
                "Failed to create bin at {shim_path}. The target {target_path} does not exist",
            );
        }
        Err(error) => {
            return Err(LinkBinsError::Chmod { path: target_path.to_path_buf(), error });
        }
    }
    Ok(true)
}

#[cfg(windows)]
pub(super) fn link_symlinked_executable<Sys>(
    _target_path: &Path,
    _shim_path: &Path,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadToString + FsEnsureExecutableBits,
{
    let _ = std::marker::PhantomData::<Sys>;
    Ok(false)
}

/// Whether the dirent at `shim_path` is a symlink that already resolves
/// to `target_path` — raw, or resolved against the bin dir — pnpm's
/// warm-install short-circuit arm for symlinked bins.
pub(super) fn symlink_already_points_at(shim_path: &Path, target_path: &Path) -> bool {
    let Ok(existing) = std::fs::read_link(shim_path) else {
        return false;
    };
    if existing == target_path {
        return true;
    }
    let Some(bins_dir) = shim_path.parent() else {
        return false;
    };
    pnpm_fs::lexical_normalize(&bins_dir.join(&existing)) == pnpm_fs::lexical_normalize(target_path)
}

/// Whether `a` and `b` are the same file. [`same_file::Handle`] proves a hard
/// link cheaply via the OS file identity (device + inode on Unix, file index +
/// volume serial on Windows). When that identity can't be obtained — a missing
/// file, or a filesystem that doesn't expose a stable index — we fall back to
/// comparing the file contents after a quick size check, which also treats a
/// byte-identical copy as the same file.
#[cfg(windows)]
fn is_same_file(a: &Path, b: &Path) -> bool {
    if let (Ok(handle_a), Ok(handle_b)) =
        (same_file::Handle::from_path(a), same_file::Handle::from_path(b))
        && handle_a == handle_b
    {
        return true;
    }
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(meta_a), Ok(meta_b)) => meta_a.len() == meta_b.len() && have_equal_contents(a, b),
        _ => false,
    }
}

/// Compare two equally-sized files chunk by chunk, so an executable is never
/// fully buffered in memory and a mismatch returns as early as possible.
#[cfg(windows)]
fn have_equal_contents(a: &Path, b: &Path) -> bool {
    const CHUNK_SIZE: usize = 64 * 1024;
    let (Ok(mut file_a), Ok(mut file_b)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let mut buf_a = vec![0u8; CHUNK_SIZE];
    let mut buf_b = vec![0u8; CHUNK_SIZE];
    loop {
        let (Ok(read_a), Ok(read_b)) =
            (read_chunk(&mut file_a, &mut buf_a), read_chunk(&mut file_b, &mut buf_b))
        else {
            return false;
        };
        if read_a != read_b {
            return false;
        }
        if read_a == 0 {
            return true;
        }
        if buf_a[..read_a] != buf_b[..read_b] {
            return false;
        }
    }
}

/// Read up to `buf.len()` bytes, looping over short reads so a full chunk is
/// only short at end of file. Like [`std::io::Read::read_exact`] but tolerant
/// of EOF.
#[cfg(windows)]
fn read_chunk(reader: &mut impl std::io::Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}
