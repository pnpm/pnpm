use crate::{rename_with_retry, retry::retry_transient_file_locks};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    fs::{self, File, OpenOptions},
    hash::{BuildHasher, Hasher},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(unix)]
use std::time::Duration;

/// POSIX `EMFILE` — process has hit `RLIMIT_NOFILE`. Hardcoded
/// instead of pulling in `libc` for a single integer that's been
/// stable across every Unix since 4.2BSD.
#[cfg(unix)]
const EMFILE: i32 = 24;

/// POSIX `ENFILE` — system-wide file table is full. Same rationale
/// as [`EMFILE`].
#[cfg(unix)]
const ENFILE: i32 = 23;

/// Run `op`, retrying on `EMFILE` / `ENFILE` with exponential
/// backoff so a transient fd-table exhaustion under heavy
/// concurrency doesn't fail the whole install. The fan-out shape
/// (many concurrent rayon workers each holding fds during CAS
/// extraction + verification) makes fd pressure likely under load.
///
/// Backoff doubles starting at 2 ms and caps at 200 ms; the budget
/// is 32 sleep-and-retry rounds followed by a final attempt (33
/// total calls) for roughly 5–6 s of total wait before we surface
/// the error. Real fd-pressure resolves in tens of ms once other
/// workers finish their writes and close fds, so we hit the cap
/// rarely.
///
/// On Windows the error codes don't map (Win32 returns its own
/// numeric space) and the runtime fd limits work differently, so
/// the helper is a thin pass-through there — the trailing `op()`
/// after the `cfg(unix)` block is the one and only attempt on that
/// platform.
pub(crate) fn retry_on_fd_pressure<Func, Value>(mut op: Func) -> io::Result<Value>
where
    Func: FnMut() -> io::Result<Value>,
{
    #[cfg(unix)]
    {
        let mut backoff = Duration::from_millis(2);
        for _ in 0..32 {
            match op() {
                Ok(value) => return Ok(value),
                Err(error) if matches!(error.raw_os_error(), Some(EMFILE | ENFILE)) => {
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_millis(200));
                }
                Err(error) => return Err(error),
            }
        }
    }
    op()
}

/// Error type of [`ensure_file`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum EnsureFileError {
    #[display("Failed to create the parent directory at {parent_dir:?}: {error}")]
    CreateDir {
        parent_dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to create file at {file_path:?}: {error}")]
    CreateFile {
        file_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to write to file at {file_path:?}: {error}")]
    WriteFile {
        file_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to read existing file at {file_path:?}: {error}")]
    ReadFile {
        file_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("Failed to rename {tmp_path:?} over {file_path:?}: {error}")]
    RenameFile {
        tmp_path: PathBuf,
        file_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Ensure `dir` (and any missing ancestors) exists. Idempotent.
///
/// Split out from [`ensure_file`] so hot-path callers (the CAFS writer)
/// can cache which directories they've already created and skip the
/// syscall cost when they have — `fs::create_dir_all` does a `stat` on
/// every call even when the directory already exists, which adds up to
/// one wasted `stat` per file on a cold install.
pub fn ensure_parent_dir(dir: &Path) -> Result<(), EnsureFileError> {
    fs::create_dir_all(dir)
        .map_err(|error| EnsureFileError::CreateDir { parent_dir: dir.to_path_buf(), error })
}

/// Write `content` to `file_path` with content-addressable-store
/// write semantics.
///
/// The parent directory must already exist. Callers that can't
/// guarantee that should call [`ensure_parent_dir`] first — splitting
/// the two lets the CAFS writer share one `create_dir_all` per shard
/// instead of paying it per file.
///
/// Sequence:
///
/// 1. Try `O_CREAT | O_EXCL` open (`OpenOptions::create_new(true)`).
///    On success we own the file and write `content` directly.
/// 2. On `ErrorKind::AlreadyExists` (warm cache or concurrent writer
///    race) re-read the file and byte-compare with `content`. CAS
///    paths are hash-derived, so matching bytes == matching digest;
///    since we already have the expected bytes in hand, comparing
///    against them verifies integrity without a separate hash step.
/// 3. If bytes match → `Ok(())`. The file is a live CAS entry; leaving
///    it alone is correct.
/// 4. If bytes mismatch, a prior install crashed mid-write and left a
///    torn blob. Recover by writing a fresh temp file next to the
///    target and `rename`ing it over. Rename is atomic on Unix
///    (`rename(2)`) and replaces-in-place on Windows
///    (`SetFileInformationByHandle`/`MoveFileEx`), so an observer
///    never sees a partial file. ([`ensure_cas_file`] instead repairs
///    in place, keeping the inode for the sake of hard-linked copies.)
/// 5. Any other open error propagates as `CreateFile`.
///
/// Design choices:
///
/// * **No upfront `stat`**: we rely on the `create_new`/`AlreadyExists`
///   signal rather than stat-then-verify, which saves one syscall
///   per file on cold installs (where every file is new) at the cost
///   of a slightly different path ordering on warm hits.
/// * **Byte-compare instead of re-hashing**: we already have the
///   buffer we were about to write, so comparing against it
///   implicitly verifies the sha512 without a second hash pass. Same
///   correctness guarantee, one fewer full-buffer walk.
/// * **Process-local per-path mutex for serialization**: two
///   snapshots whose tarballs ship identical file content
///   (e.g. a shared `LICENSE`) compute the same CAS path and would
///   race in `verify_or_rewrite`. The mutex makes the second
///   writer wait for the first's `write_all` so the byte-match
///   fast path always applies. See [`cas_write_lock`].
///
/// Guarantee: a successful return means `file_path` exists on disk
/// with contents equal to `content`. A torn mid-write from a previous
/// install is self-healing, not persistent.
pub fn ensure_file(
    file_path: &Path,
    content: &[u8],
    #[cfg_attr(windows, allow(unused, reason = "POSIX mode bits are only applied on Unix"))]
    mode: Option<u32>,
) -> Result<(), EnsureFileError> {
    ensure(file_path, content, mode, Repair::Rename)
}

/// [`ensure_file`] with the repair strategy CAS blobs need: when the
/// existing file's bytes mismatch, overwrite it in place first, keeping
/// the inode so the hard links to it from other projects'
/// `node_modules` are healed by the same write (pnpm/pnpm#3445). The
/// temp+`rename` repair [`ensure_file`] uses would swap the inode and
/// leave those copies corrupt. Falls back to the rename when the
/// in-place overwrite is refused or fails verification.
///
/// In-place overwrite is not atomic — a concurrent reader can observe
/// torn content for the duration of the write — so this variant is for
/// content-addressed blobs only: their consumers validate integrity and
/// re-trigger this repair on a torn read. Files whose readers take the
/// bytes as-is (`.pnp.cjs`, the package map) must keep [`ensure_file`]'s
/// atomic rename.
pub fn ensure_cas_file(
    file_path: &Path,
    content: &[u8],
    #[cfg_attr(windows, allow(unused, reason = "POSIX mode bits are only applied on Unix"))]
    mode: Option<u32>,
) -> Result<(), EnsureFileError> {
    ensure(file_path, content, mode, Repair::InPlace)
}

/// How [`ensure`] repairs an existing file whose bytes mismatch.
#[derive(Clone, Copy)]
enum Repair {
    /// Temp file + `rename` over the target. Atomic, but swaps the
    /// inode, disconnecting hard-linked copies.
    Rename,
    /// Truncate and rewrite under the same inode, healing hard-linked
    /// copies; falls back to the rename when refused.
    InPlace,
}

fn ensure(
    file_path: &Path,
    content: &[u8],
    mode: Option<u32>,
    repair: Repair,
) -> Result<(), EnsureFileError> {
    // See the "Process-local per-path mutex" bullet above and
    // [`cas_write_lock`] for the rationale.
    let lock = cas_write_lock(file_path);
    let _guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(mode) = mode {
            options.mode(mode);
        }
    }

    match retry_on_fd_pressure(|| options.open(file_path)) {
        Ok(mut file) => file
            .write_all(content)
            .map_err(|error| EnsureFileError::WriteFile {
                file_path: file_path.to_path_buf(),
                error,
            }),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            verify_or_rewrite(file_path, content, mode, repair)
        }
        Err(error) => {
            Err(EnsureFileError::CreateFile { file_path: file_path.to_path_buf(), error })
        }
    }
}

/// Borrow the process-local write mutex for `file_path`.
///
/// The hot path costs one path hash + one uncontended mutex acquire
/// per CAFS file written (~170k on the alotta-files fixture), with no
/// allocations: the path is hashed into one of `NUM_CAS_LOCK_STRIPES`
/// statically-allocated mutexes.
///
/// **Coordination contract.** Callers handing in the same `&Path`
/// always receive the same `Mutex<()>`. The hasher is initialised
/// once per process (`LazyLock<RandomState>`) so the path-to-stripe
/// mapping stays stable for the lifetime of the process; writers
/// ([`ensure_file`]) and verifiers (`check_pkg_files_integrity`) of the
/// same path are guaranteed to meet on the same lock and serialise.
/// Stripe-hash collisions between unrelated paths block each other
/// too — that false-sharing is bounded by `NUM_CAS_LOCK_STRIPES` and
/// the guarded section (a single `O_CREAT|O_EXCL` open + `write_all`)
/// is microseconds long.
///
/// Made `pub` so verifiers (`check_pkg_files_integrity`) can acquire
/// the same lock before stat-then-maybe-`rimraf`'ing a CAS path —
/// otherwise the verifier can `unlink` a file while a writer's
/// `write_all` is still running.
pub fn cas_write_lock(file_path: &Path) -> &'static Mutex<()> {
    use std::collections::hash_map::RandomState;
    static BUILDER: std::sync::LazyLock<RandomState> = std::sync::LazyLock::new(RandomState::new);
    let mut hasher = BUILDER.build_hasher();
    std::hash::Hash::hash(file_path, &mut hasher);
    let stripe = (hasher.finish() as usize) & (NUM_CAS_LOCK_STRIPES - 1);
    &CAS_LOCK_STRIPES[stripe]
}

/// Number of static mutex stripes used by [`cas_write_lock`]. Power of
/// two so the modulo collapses to a mask. 256 picked so each stripe
/// sees on average `total_files / 256` writes per install; for a 170k-
/// file install that's ~660 writes per stripe, all on different paths
/// — uncontended pairings dominate.
const NUM_CAS_LOCK_STRIPES: usize = 256;
const _: () = assert!(
    NUM_CAS_LOCK_STRIPES.is_power_of_two(),
    "cas_write_lock uses `& (NUM_CAS_LOCK_STRIPES - 1)` as the stripe selector, which only \
     distributes uniformly when the count is a power of two",
);

static CAS_LOCK_STRIPES: [Mutex<()>; NUM_CAS_LOCK_STRIPES] =
    [const { Mutex::new(()) }; NUM_CAS_LOCK_STRIPES];

/// Re-read an already-present CAS file and byte-compare with `content`.
/// If they match we're done; if not, recover the torn blob by writing a
/// fresh temp file and renaming it over the target.
///
/// Uses `symlink_metadata` (not `metadata`) first to reject the
/// non-regular-file cases — symlinks in particular. On Unix,
/// `open(O_CREAT|O_EXCL)` returns `EEXIST` even when the dirent is
/// a symlink (POSIX `open` does not follow symlinks under `O_EXCL`),
/// so a tampered / backed-up-and-restored store could route a symlinked
/// dirent into this function. If we fell through directly to `fs::read`
/// (which *does* follow symlinks), a symlink pointing at a file with
/// matching bytes would silently return `Ok(())` without ever
/// materialising a real CAS blob at `file_path`, and downstream
/// `fs::hard_link` on that path would hardlink the symlink itself
/// rather than the target. Scrub instead: [`write_atomic`]'s `rename`
/// atomically replaces the symlink (or any other non-regular dirent
/// that `rename` can overwrite) with a real regular file. Pacquet's
/// CAS linking path is strict about file-type, so the guard is worth
/// adding here.
///
/// A `NotFound` on either syscall means the dirent disappeared
/// between our `create_new` attempt and the metadata / read call —
/// another process cleaned it up (unusual, but possible in shared-
/// store setups). Fall through to the atomic-write path, which will
/// re-create it.
fn verify_or_rewrite(
    file_path: &Path,
    content: &[u8],
    mode: Option<u32>,
    repair: Repair,
) -> Result<(), EnsureFileError> {
    match fs::symlink_metadata(file_path) {
        Ok(meta) if !meta.file_type().is_file() => {
            // Symlink, directory, fifo, socket, block/char device —
            // not a regular CAS blob. Scrub via atomic rewrite.
            write_atomic(file_path, content, mode)
        }
        // Cheap size-mismatch reject before we read a single byte —
        // a CAS file whose length doesn't match the buffer we were
        // about to write cannot possibly have matching contents.
        Ok(meta) if meta.len() != content.len() as u64 => {
            repair_file(file_path, content, mode, repair)
        }
        Ok(_) => match file_equals_bytes(file_path, content) {
            Ok(true) => Ok(()),
            Ok(false) => repair_file(file_path, content, mode, repair),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_atomic(file_path, content, mode)
            }
            Err(error) => {
                Err(EnsureFileError::ReadFile { file_path: file_path.to_path_buf(), error })
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            write_atomic(file_path, content, mode)
        }
        Err(error) => Err(EnsureFileError::ReadFile { file_path: file_path.to_path_buf(), error }),
    }
}

/// Repair a corrupt regular file at `file_path` per the caller's
/// strategy. [`Repair::InPlace`] overwrites under the same inode so
/// every hard link to the file — other projects' `node_modules` copies
/// of the CAS blob — is healed by the same write (pnpm/pnpm#3445),
/// falling back to [`write_atomic`]'s temp+rename when the in-place
/// overwrite is refused or the freshly written bytes fail verification
/// (e.g. a concurrent process still mid-write on the same path
/// interleaved with ours).
fn repair_file(
    file_path: &Path,
    content: &[u8],
    mode: Option<u32>,
    repair: Repair,
) -> Result<(), EnsureFileError> {
    let mut source = content;
    let repaired = matches!(repair, Repair::InPlace)
        && overwrite_file_in_place(file_path, &mut source)
        && file_equals_bytes(file_path, content).unwrap_or(false);
    if repaired { Ok(()) } else { write_atomic(file_path, content, mode) }
}

/// Stream `file_path` and byte-compare against `content` without
/// buffering the whole file in memory.
///
/// Reading the whole file into a `Vec<u8>` would allocate the size of
/// the file; on a CAS entry for a large binary (10–30 MB isn't unusual
/// in `@napi-rs/*`, `esbuild`, etc.) and many concurrent rayon workers
/// hitting this branch, that allocation stacks up. Streaming in 8 KB
/// chunks holds a fixed stack buffer regardless of file size.
///
/// Any chunk mismatch returns `Ok(false)` immediately — we don't
/// finish reading the file once we know it differs. An
/// `UnexpectedEof` from `read_exact` is returned as `Ok(false)` too:
/// the file shrunk under us (another process truncated it or the
/// metadata was stale), which by definition means its contents don't
/// match `content`. Other errors propagate.
fn file_equals_bytes(file_path: &Path, content: &[u8]) -> io::Result<bool> {
    use std::io::Read;

    let mut file = retry_on_fd_pressure(|| File::open(file_path))?;
    let mut buf = [0u8; 8 * 1024];
    let mut offset = 0;

    while offset < content.len() {
        let chunk_len = (content.len() - offset).min(buf.len());
        match file.read_exact(&mut buf[..chunk_len]) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        }
        if buf[..chunk_len] != content[offset..offset + chunk_len] {
            return Ok(false);
        }
        offset += chunk_len;
    }

    // Confirm the file ends where `content` ends — if there's a
    // trailing byte the size-check earlier missed (shouldn't happen
    // given the size-match guard in `verify_or_rewrite`, but cheap
    // to assert), treat it as not-equal.
    let mut overflow = [0u8; 1];
    match file.read(&mut overflow) {
        Ok(0) => Ok(true),
        Ok(_) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Overwrite the regular file at `file_path` in place with bytes from
/// `reader`, keeping the inode so hard-linked copies of the file — other
/// projects' `node_modules` entries importing the same CAS blob — are
/// healed by the same write (pnpm/pnpm#3445).
///
/// The path is rejected unless it is a regular file, and the opened
/// handle is verified to be that same file before a byte is written:
/// on Unix the open uses `O_NOFOLLOW`, so a symlink swapped in after
/// the metadata check is refused rather than followed, but Windows has
/// no such flag, so the descriptor itself is compared against the
/// checked dirent's identity. `O_NONBLOCK` keeps a FIFO from holding
/// the open.
///
/// Returns `false` when in-place overwrite is refused and the caller
/// should fall back to an atomic temp+rename: the target is not a
/// regular file, it refuses the write open even after its write
/// protection is lifted (a running executable's `ETXTBSY`, another
/// owner's file), or the write failed. Every such state is one the
/// rename handles correctly, and a persistent failure (e.g. `ENOSPC`)
/// re-surfaces with proper context when the fallback attempts its own
/// write, so no error detail is lost by collapsing these into `false`.
///
/// In-place overwrite is not atomic: a concurrent reader can observe
/// torn content for the duration of the write. The file was already
/// corrupt, and a failed integrity check re-triggers this repair, so
/// the trade is a brief torn-read window for healing every hard-linked
/// copy at once.
pub fn overwrite_file_in_place(file_path: &Path, reader: &mut dyn io::Read) -> bool {
    #[cfg(unix)]
    let expected = match fs::symlink_metadata(file_path) {
        Ok(meta) if meta.file_type().is_file() => meta,
        _ => return false,
    };
    let mut options = OpenOptions::new();
    options.write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let Some((mut file, restore_permissions)) = open_for_overwrite(file_path, &options) else {
        return false;
    };
    #[cfg(unix)]
    let handle_matches = same_file(&file, &expected);
    #[cfg(windows)]
    let handle_matches = same_file(&file, file_path);
    let written = handle_matches && io::copy(reader, &mut file).is_ok();
    drop(file);
    if let Some(permissions) = restore_permissions {
        // Best-effort restore; the next repair retries.
        let _ = fs::set_permissions(file_path, permissions);
    }
    written
}

/// Whether the opened handle is the same regular file `expected`
/// describes — the guard against a symlink swapped into the path
/// between the metadata check and the open, which a write open would
/// otherwise follow into a file the store does not own.
#[cfg(unix)]
fn same_file(file: &File, expected: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    file.metadata()
        .is_ok_and(|handle_meta| {
            handle_meta.file_type().is_file()
                && handle_meta.dev() == expected.dev()
                && handle_meta.ino() == expected.ino()
        })
}

#[cfg(windows)]
fn same_file(file: &File, file_path: &Path) -> bool {
    let (Ok(handle_a), Ok(handle_b)) = (
        file.try_clone().and_then(same_file::Handle::from_file),
        same_file::Handle::from_path(file_path),
    ) else {
        return false;
    };
    handle_a == handle_b
}

/// Open a store blob for truncate-and-rewrite, returning the handle and
/// the permissions to restore after writing. When the blob is
/// write-protected, lift the protection before writing: tar entries keep
/// modes like `0o444`, and the readonly attribute that maps to on
/// Windows refuses a write open with `ERROR_ACCESS_DENIED`. The repair
/// changes the file's content, not its protection, so the saved
/// permissions go back on afterwards.
///
/// The open runs under [`retry_transient_file_locks`]: antivirus and
/// indexer scans briefly hold just-written Windows paths open, failing
/// an unlucky open with an access-denied error that clears moments
/// later. Lifting write protection beforehand avoids consuming the
/// Windows permission-denied retry budget on a read-only file.
fn open_for_overwrite(
    file_path: &Path,
    options: &OpenOptions,
) -> Option<(File, Option<fs::Permissions>)> {
    let meta = fs::symlink_metadata(file_path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let restore_permissions = if is_write_protected(&meta.permissions()) {
        fs::set_permissions(file_path, make_writable(&meta.permissions())).ok()?;
        Some(meta.permissions())
    } else {
        None
    };
    let open = || retry_transient_file_locks(|| retry_on_fd_pressure(|| options.open(file_path)));
    if let Ok(file) = open() {
        return Some((file, restore_permissions));
    }
    if let Some(ref permissions) = restore_permissions {
        // Best-effort restore; the repair falls back to temp+rename.
        let _ = fs::set_permissions(file_path, permissions.clone());
    }
    None
}

/// Whether the file is write-protected for the current process: on Unix
/// by lacking the owner-write bit, and on Windows by having the readonly
/// attribute. Rust's `readonly()` checks whether all write bits (owner,
/// group, other) are clear, which returns false for modes like 0o464
/// where the owner still cannot write.
#[cfg(unix)]
fn is_write_protected(permissions: &fs::Permissions) -> bool {
    use std::os::unix::fs::PermissionsExt;
    permissions.mode() & 0o200 == 0
}

#[cfg(windows)]
fn is_write_protected(permissions: &fs::Permissions) -> bool {
    permissions.readonly()
}

/// Grant write permission, disturbing nothing else: on Unix by adding
/// the owner-write bit to the mode — a blanket `set_readonly(false)`
/// would make the file world-writable — and on Windows by clearing the
/// readonly attribute, the only permission Windows tracks.
#[cfg(unix)]
fn make_writable(permissions: &fs::Permissions) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(permissions.mode() | 0o200)
}

#[cfg(windows)]
// The lint guards the Unix world-writable side effect; on Windows
// clearing the readonly attribute is the exact operation.
#[allow(clippy::permissions_set_readonly_false)]
fn make_writable(permissions: &fs::Permissions) -> fs::Permissions {
    let mut writable = permissions.clone();
    writable.set_readonly(false);
    writable
}

/// Write `content` to a unique temporary path next to `file_path` and
/// `rename` it over the target. The rename is the only atomic step; an
/// observer sees either the old contents or the new ones, never a
/// half-written blob.
///
/// The temp file itself is opened with `O_CREAT|O_EXCL`
/// (`create_new(true)`) rather than `create+truncate` so we never
/// follow a symlink or truncate a file an attacker (or a crashed
/// prior install) pre-seeded at our predicted temp path. If we hit
/// `AlreadyExists` anyway — collisions are vanishingly rare given the
/// pid + per-process atomic counter temp scheme, but cross-container
/// shared-store setups can re-use pids — we advance the counter and
/// try again, up to `MAX_TEMP_ATTEMPTS` times.
///
/// Open errors are classified as `CreateFile`; write errors as
/// `WriteFile`. On any failure the partially-created temp file is
/// removed best-effort so stale files don't leak into the store
/// shard.
fn write_atomic(
    file_path: &Path,
    content: &[u8],
    mode: Option<u32>,
) -> Result<(), EnsureFileError> {
    let parent = file_path.parent().unwrap_or_else(|| Path::new("."));
    let name = file_path
        .file_name()
        .map(|file_name| file_name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (tmp_path, mut file) = create_exclusive_temp_file(parent, &strip_dash_suffix(&name), mode)?;

    if let Err(error) = file.write_all(content) {
        drop(file);
        let _ = fs::remove_file(&tmp_path);
        return Err(EnsureFileError::WriteFile { file_path: tmp_path, error });
    }
    // Close the handle before `rename`. Windows `MoveFileEx` over
    // an open source file can fail with sharing-violation; Unix
    // doesn't care but an early `close` lets the kernel commit
    // dirty buffers before the rename commits the dirent change.
    drop(file);

    if let Err(error) = rename_with_retry(&tmp_path, file_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(EnsureFileError::RenameFile {
            tmp_path,
            file_path: file_path.to_path_buf(),
            error,
        });
    }
    Ok(())
}

/// Create a uniquely-named file inside `dir` with `O_CREAT | O_EXCL`
/// semantics, returning its path and open handle. The name is
/// `{base}{pid}{counter}`: the counter is a process-local
/// monotonically-increasing atomic, giving uniqueness across rayon /
/// tokio workers in the same process, and the pid avoids collisions
/// when multiple install processes share a store dir.
///
/// The exclusive open means we never follow a symlink or truncate a
/// file an attacker (or a crashed prior install) pre-seeded at our
/// predicted temp path. If we hit `AlreadyExists` anyway — collisions
/// are vanishingly rare given the pid + per-process atomic counter temp
/// scheme, but cross-container shared-store setups can re-use pids — we
/// advance the counter and try again, up to `MAX_TEMP_ATTEMPTS` times.
///
/// The caller owns the file's lifecycle: rename it into place on
/// success, remove it on failure.
pub fn create_exclusive_temp_file(
    dir: &Path,
    base: &str,
    // `mode` feeds `OpenOptionsExt::mode` inside the `cfg(unix)` block
    // below; Windows has no POSIX mode bits to set at open time, so the
    // parameter is genuinely unused there.
    #[cfg_attr(windows, allow(unused, reason = "POSIX mode bits are only applied on Unix"))]
    mode: Option<u32>,
) -> Result<(PathBuf, File), EnsureFileError> {
    /// Retries after `AlreadyExists` on the temp path. Sixteen fresh
    /// counter values is plenty — under benign conditions we never
    /// collide; under shared-store-across-containers the chance of
    /// 16 consecutive same-pid same-counter collisions is negligible.
    const MAX_TEMP_ATTEMPTS: usize = 16;

    let mut last_already_exists: Option<io::Error> = None;

    for _ in 0..MAX_TEMP_ATTEMPTS {
        let tmp_path = temp_path_in(dir, base);

        let mut options = OpenOptions::new();
        options.write(true).create_new(true);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if let Some(mode) = mode {
                options.mode(mode);
            }
        }

        match retry_on_fd_pressure(|| options.open(&tmp_path)) {
            Ok(file) => return Ok((tmp_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Stale temp file or adversarial / concurrent pre-seed.
                // Retry with a fresh counter; don't touch whatever is
                // at the colliding path.
                last_already_exists = Some(error);
            }
            Err(error) => {
                return Err(EnsureFileError::CreateFile { file_path: tmp_path, error });
            }
        }
    }

    // Ran out of temp-name attempts. Surface the last `AlreadyExists`
    // so the operator can see what happened; pick the directory as
    // the best-effort context since we can't enumerate every temp
    // name we tried.
    Err(EnsureFileError::CreateFile {
        file_path: dir.to_path_buf(),
        error: last_already_exists.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "exhausted temp-path attempts for exclusive temp file",
            )
        }),
    })
}

/// Build a unique temp path inside `dir`, of the form
/// `{base}{pid}{counter}` per [`create_exclusive_temp_file`]'s
/// uniqueness contract.
fn temp_path_in(dir: &Path, base: &str) -> PathBuf {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();

    dir.join(format!("{base}{pid}{counter}"))
}

/// Strip the first `-…` tail; if the tail was `-exec`, append `x`. On
/// pacquet's CAS names (`{hex}` or `{hex}-exec`) the only real input is
/// those two shapes, but the general form is handled so any future
/// suffix doesn't silently diverge. Applied to [`write_atomic`]'s temp
/// names mainly so temp files don't look like executable CAS entries to
/// any observer scanning the shard.
fn strip_dash_suffix(name: &str) -> String {
    let Some(dash_pos) = name.find('-') else {
        return name.to_string();
    };
    let without_suffix = &name[..dash_pos];
    if &name[dash_pos..] == "-exec" {
        format!("{without_suffix}x")
    } else {
        without_suffix.to_string()
    }
}

#[cfg(test)]
mod tests;
