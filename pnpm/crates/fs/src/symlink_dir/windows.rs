use derive_more::{Display, Error};
use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};

/// Cached choice of writer. `UNDECIDED` until the first successful
/// call resolves the EPERM probe; afterward [`USE_SYMLINK`] or
/// [`USE_JUNCTION`]. Caching the winning branch after the first call
/// avoids re-probing on every subsequent symlink.
const UNDECIDED: u8 = 0;
const USE_SYMLINK: u8 = 1;
const USE_JUNCTION: u8 = 2;
static MODE: AtomicU8 = AtomicU8::new(UNDECIDED);
static JUNCTION_STAGING_ID: AtomicU64 = AtomicU64::new(0);
static JUNCTION_COMMIT_LOCK: Mutex<()> = Mutex::new(());

/// True symlinks on Windows take a relative target —
/// `path.relative(dirname(dest), src)`, the same form used on Unix.
/// Junctions take the absolute path with a trailing backslash, but
/// the `junction` crate handles that internally so we pass
/// `original` through unchanged for the junction branch.
pub fn create(original: &Path, link: &Path) -> io::Result<()> {
    create_with_contents(original, &super::relative_target_for(original, link), link)
}

/// [`create`] with a true symlink holding `original` as given. A
/// junction holds the absolute path either way.
pub fn create_absolute(original: &Path, link: &Path) -> io::Result<()> {
    create_with_contents(original, original, link)
}

pub(super) fn create_with_contents(
    original: &Path,
    contents: &Path,
    link: &Path,
) -> io::Result<()> {
    create_link(original, contents, link)
        .map_err(|error| explain_network_share_refusal(link, error))
}

/// A link on a network share that could not be created. Many SMB servers,
/// Windows ones included, deny both the symlink and the junction, and
/// pnpm 11 fails there in the same way. The share's permissions can deny
/// it too, so the message doesn't claim to know which. The hoisted linker
/// still links `link:` and `workspace:` dependencies, so the advice
/// doesn't promise it avoids every link. The refusal stays reachable as
/// the source.
#[derive(Debug, Display, Error)]
#[display(
    "Could not create a link on a network share. Windows file shares often refuse junctions \
     and symlinks. If this one does, move the project to a local drive, or set `nodeLinker: \
     hoisted` in pnpm-workspace.yaml, which copies registry dependencies into node_modules \
     instead of linking them. Workspace packages are linked even with the hoisted linker."
)]
pub(super) struct NetworkShareLinkError {
    source: io::Error,
}

/// Keep the kind of a refused link, but say what to do about it when the
/// link is on a network share.
pub(super) fn explain_network_share_refusal(link: &Path, error: io::Error) -> io::Error {
    let refused =
        matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported);
    if !refused || !crate::mapped_drive_path::is_on_network_drive(link) {
        return error;
    }
    io::Error::new(error.kind(), NetworkShareLinkError { source: error })
}

fn create_link(original: &Path, contents: &Path, link: &Path) -> io::Result<()> {
    // Windows evaluates a symlink on a share on the client, and by default
    // doesn't follow one there that points back into a share ("remote to
    // remote" in `fsutil behavior query SymlinkEvaluation`). The relative
    // links in `node_modules/.pnpm` are exactly that, so a share that accepts
    // symlinks would get links nothing can open. A junction is resolved by
    // the server, and pnpm 11 links with junctions everywhere on Windows, so
    // links on a share stay junctions whatever the process may create. This
    // doesn't touch `MODE`: links on local drives keep preferring symlinks.
    if crate::mapped_drive_path::is_on_network_drive_cached(link) {
        return create_junction(original, link);
    }
    match MODE.load(Ordering::Relaxed) {
        USE_SYMLINK => match std::os::windows::fs::symlink_dir(contents, link) {
            Err(error) if should_fallback_to_junction(&error) => {
                create_and_cache_junction(original, link)
            }
            result => result,
        },
        USE_JUNCTION => create_junction(original, link),
        _ => probe_and_cache(original, contents, link),
    }
}

/// Whether `link` is a true symlink on a network share. [`create_link`]
/// never writes one, but an earlier pnpm 12 install may have, and Windows
/// doesn't follow it by default, so it is replaced rather than reused even
/// when it points at the right target.
pub(super) fn is_symlink_on_network_share(link: &Path) -> bool {
    crate::mapped_drive_path::is_on_network_drive_cached(link)
        && reparse_tag(link) == Some(IO_REPARSE_TAG_SYMLINK)
}

pub(super) const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// [`fs::read_link`] for a junction only: it fails for a true symlink or
/// any other entry, with the same open and reparse read.
///
/// `junction::get_target` strips only the `\??\` prefix, so a junction to
/// `\\server\share\…`, held as `\??\UNC\server\share\…`, comes back as
/// `UNC\server\share\…`. That is turned back into the UNC path, which is
/// what [`fs::read_link`] returns for it.
pub(super) fn read_junction_target(link: &Path) -> io::Result<PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt as _, OsStringExt as _},
    };

    let target = junction::get_target(link)?;
    let wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .collect();
    let unc_prefix: Vec<u16> = r"UNC\".encode_utf16().collect();
    let Some(rest) = wide.strip_prefix(unc_prefix.as_slice()) else {
        return Ok(target);
    };
    let mut unc: Vec<u16> = r"\\".encode_utf16().collect();
    unc.extend_from_slice(rest);
    Ok(PathBuf::from(OsString::from_wide(&unc)))
}

/// The reparse tag of `path` itself, or `None` when it isn't a reparse
/// point or can't be read. The standard library reports a junction and a
/// symlink alike as a symlink, so the tag is the only way to tell them apart.
pub(super) fn reparse_tag(path: &Path) -> Option<u32> {
    use std::os::windows::{fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FileAttributeTagInfo,
        GetFileInformationByHandleEx,
    };

    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut info = FILE_ATTRIBUTE_TAG_INFO { FileAttributes: 0, ReparseTag: 0 };
    // SAFETY: `info` is writable for the size passed, and the handle stays
    // open for the call.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    (ok != 0 && info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0).then_some(info.ReparseTag)
}

fn probe_and_cache(original: &Path, contents: &Path, link: &Path) -> io::Result<()> {
    // Try the true directory symlink first — that's what users
    // running in Developer Mode (or as Administrator) get, and
    // true symlinks are preferred over junctions when allowed.
    // `CreateSymbolicLinkW` returns
    // `ERROR_PRIVILEGE_NOT_HELD` when the process can't create
    // symlinks; junctions don't carry that constraint, so fall
    // back to those.
    match std::os::windows::fs::symlink_dir(contents, link) {
        Ok(()) => {
            MODE.store(USE_SYMLINK, Ordering::Relaxed);
            Ok(())
        }
        Err(error) if should_fallback_to_junction(&error) => {
            create_and_cache_junction(original, link)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn should_fallback_to_junction(error: &io::Error) -> bool {
    const ERROR_DIRECTORY: i32 = 267;
    // `CreateSymbolicLinkW` without symlink privilege (no Developer
    // Mode, not elevated) fails with `ERROR_PRIVILEGE_NOT_HELD`,
    // which std maps to `Uncategorized` — not `PermissionDenied` —
    // so it must be matched by raw os error.
    const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
    error.kind() == io::ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(ERROR_DIRECTORY | ERROR_PRIVILEGE_NOT_HELD))
}

fn create_and_cache_junction(original: &Path, link: &Path) -> io::Result<()> {
    let result = create_junction(original, link);
    if result.is_ok() {
        MODE.store(USE_JUNCTION, Ordering::Relaxed);
    }
    result
}

pub(super) fn create_junction(original: &Path, link: &Path) -> io::Result<()> {
    let staging = stage_junction(&junction_target(original), link)?;
    commit_staged_junction(link, &staging)
}

/// The target to give `junction::create` for `original`.
///
/// The `junction` crate prefixes the full path with `\??\`, which makes an
/// NT path only of a drive path or a verbatim path. A plain UNC path
/// `\\server\share\…` would become `\??\\\server\share\…`, which names
/// nothing. Its verbatim form `\\?\UNC\server\share\…` becomes
/// `\??\UNC\server\share\…`, the target Node.js writes for a junction to
/// that path. A relative `original` is resolved first, as the crate would
/// resolve it, because under a UNC working directory it becomes a UNC path.
pub(super) fn junction_target(original: &Path) -> Cow<'_, Path> {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt as _, OsStringExt as _},
        path::{Component, Prefix},
    };

    if original.is_relative()
        && let Ok(absolute) = std::path::absolute(original)
    {
        return match junction_target(&absolute) {
            Cow::Borrowed(_) => Cow::Owned(absolute),
            Cow::Owned(verbatim) => Cow::Owned(verbatim),
        };
    }
    let is_plain_unc = matches!(
        original.components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::UNC(..)),
    );
    if !is_plain_unc {
        return Cow::Borrowed(original);
    }
    let mut verbatim: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    verbatim.extend(
        original
            .as_os_str()
            .encode_wide()
            .skip(2),
    );
    Cow::Owned(PathBuf::from(OsString::from_wide(&verbatim)))
}

/// Build the junction at a unique sibling path so no other worker can
/// observe the `junction` crate's transient plain directory at `link`.
fn stage_junction(original: &Path, link: &Path) -> io::Result<PathBuf> {
    loop {
        let candidate = junction_staging_path(link);
        match junction::create(original, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(discard_failed_staging(&candidate, error)),
        }
    }
}

/// `junction::create` makes the directory before it sets the reparse
/// point, and leaves it behind when that later step fails. Remove it so a
/// failed link doesn't leave a `.pnpm-junction-*` directory next to it.
/// `remove_dir` only removes an empty directory, and `NotFound` means the
/// directory was never created. The error keeps its kind either way,
/// because callers branch on it.
fn discard_failed_staging(candidate: &Path, error: io::Error) -> io::Error {
    match fs::remove_dir(candidate) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => error,
        Err(cleanup) => io::Error::new(
            error.kind(),
            format!("{error}; staged junction cleanup failed at {candidate:?}: {cleanup}"),
        ),
    }
}

/// Publish `staging` at `link` with an atomic rename, folding a lost race
/// into the `AlreadyExists` reuse signal. A transient Windows file lock
/// on inspection or rename is retried, one [`attempt_commit`] per try.
fn commit_staged_junction(link: &Path, staging: &Path) -> io::Result<()> {
    let commit_error = match super::retry_transient_file_locks(|| attempt_commit(link, staging)) {
        Ok(CommitAttempt::Committed) => return Ok(()),
        Ok(CommitAttempt::DestinationTaken) => {
            return Err(reuse_completed_destination(link, staging, ""));
        }
        Ok(CommitAttempt::InspectFailed(error)) => {
            return Err(inspect_failed(link, staging, &error, ""));
        }
        Err(error) => error,
    };

    // The commit either lost a cross-process race or genuinely failed;
    // re-inspecting the destination tells those apart.
    let after_commit = format!(" after a failed commit ({commit_error})");
    match inspect_destination(link) {
        Destination::Exists => Err(reuse_completed_destination(link, staging, &after_commit)),
        Destination::InspectFailed(error) => {
            Err(inspect_failed(link, staging, &error, &after_commit))
        }
        Destination::Missing => Err(discard_staging_after_commit(staging, link, commit_error)),
    }
}

/// Outcome of one serialized attempt to publish a staged junction.
enum CommitAttempt {
    Committed,
    DestinationTaken,
    InspectFailed(io::Error),
}

/// One try at publishing a staged junction under [`JUNCTION_COMMIT_LOCK`].
///
/// Holding the lock for a single inspect-and-rename, rather than across
/// the retries in [`commit_staged_junction`], keeps one locked path from
/// stalling every other junction commit in the process and leaves the
/// slow part — the reparse-point conversion inside [`stage_junction`] —
/// running in parallel. Re-inspecting the destination on every try means
/// a race lost while waiting is reused rather than retried through the
/// budget. A delete-pending destination can deny inspection temporarily;
/// that error follows the same bounded retry policy as a locked rename.
fn attempt_commit(link: &Path, staging: &Path) -> io::Result<CommitAttempt> {
    let _commit_guard = JUNCTION_COMMIT_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    match inspect_destination(link) {
        Destination::Missing => {}
        Destination::Exists => return Ok(CommitAttempt::DestinationTaken),
        Destination::InspectFailed(error) if crate::retry::is_transient_file_lock_error(&error) => {
            return Err(error);
        }
        Destination::InspectFailed(error) => return Ok(CommitAttempt::InspectFailed(error)),
    }
    fs::rename(staging, link).map(|()| CommitAttempt::Committed)
}

/// What `symlink_metadata` reports about a would-be junction destination.
enum Destination {
    /// Another worker already committed the link.
    Exists,
    /// The slot is free to claim.
    Missing,
    InspectFailed(io::Error),
}

fn inspect_destination(link: &Path) -> Destination {
    match fs::symlink_metadata(link) {
        Ok(_) => Destination::Exists,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Destination::Missing,
        Err(error) => Destination::InspectFailed(error),
    }
}

/// The destination already holds a completed junction. Discard our staging
/// copy and return `AlreadyExists` so [`super::force_symlink_inner`] reuses
/// the finished link. A cleanup failure must not suppress that reuse — it
/// rides along as a [`super::ConcurrentCleanupWarning`] so the orphaned
/// staging path still reaches the user.
fn reuse_completed_destination(link: &Path, staging: &Path, context: &str) -> io::Error {
    match fs::remove_dir(staging) {
        Ok(()) => io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("junction path already exists{context}: {link:?}"),
        ),
        Err(cleanup) => io::Error::new(
            io::ErrorKind::AlreadyExists,
            super::ConcurrentCleanupWarning(format!(
                "junction path already exists{context} at {link:?}, but staged junction \
                 cleanup failed at {staging:?}: {cleanup}",
            )),
        ),
    }
}

/// The destination couldn't be inspected. Discard staging and surface the
/// inspection error, never as `AlreadyExists` — that kind would let the
/// caller reuse a link we could not verify. A concurrent cleanup failure
/// downgrades the kind to `Other` since two independent errors are merged.
fn inspect_failed(
    link: &Path,
    staging: &Path,
    inspect_error: &io::Error,
    context: &str,
) -> io::Error {
    match fs::remove_dir(staging) {
        Ok(()) => io::Error::new(
            inspect_error.kind(),
            format!("failed to inspect junction destination {link:?}{context}: {inspect_error}"),
        ),
        Err(cleanup) => io::Error::other(format!(
            "failed to inspect junction destination {link:?}{context}: {inspect_error}; \
             staged-junction cleanup failed: {cleanup}",
        )),
    }
}

/// The commit genuinely failed and left nothing at `link`. Discard staging
/// and surface the commit error — but never as `AlreadyExists`, or
/// [`super::force_symlink_inner`] would treat the missing link as reusable.
/// A rename that lost the race only to have the winner vanish before the
/// re-inspection can carry that kind, so strip it to `Other`; every other
/// kind (including `NotFound`, which drives a mkdir + retry) is informative
/// and safe to surface unchanged.
pub(super) fn discard_staging_after_commit(
    staging: &Path,
    link: &Path,
    commit_error: io::Error,
) -> io::Error {
    match fs::remove_dir(staging) {
        Ok(()) if commit_error.kind() != io::ErrorKind::AlreadyExists => commit_error,
        Ok(()) => io::Error::other(format!(
            "failed to commit staged junction {staging:?} to {link:?}: {commit_error}",
        )),
        Err(cleanup) => io::Error::other(format!(
            "failed to commit staged junction {staging:?} to {link:?}: {commit_error}; \
             cleanup failed: {cleanup}",
        )),
    }
}

fn junction_staging_path(link: &Path) -> PathBuf {
    let id = JUNCTION_STAGING_ID.fetch_add(1, Ordering::Relaxed);
    let name = format!(".pnpm-junction-{}-{id}", crate::process_id());
    link.with_file_name(name)
}
