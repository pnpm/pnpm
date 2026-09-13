use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};

/// Cached choice of writer. `UNDECIDED` until the first successful
/// call resolves the EPERM probe; afterward `USE_SYMLINK` or
/// `USE_JUNCTION`. Caching the winning branch after the first call
/// avoids re-probing on every subsequent symlink.
const UNDECIDED: u8 = 0;
const USE_SYMLINK: u8 = 1;
const USE_JUNCTION: u8 = 2;
static MODE: AtomicU8 = AtomicU8::new(UNDECIDED);
static JUNCTION_STAGING_ID: AtomicU64 = AtomicU64::new(0);
static JUNCTION_COMMIT_LOCK: Mutex<()> = Mutex::new(());

pub fn create(original: &Path, link: &Path) -> io::Result<()> {
    match MODE.load(Ordering::Relaxed) {
        USE_SYMLINK => match create_true_symlink(original, link) {
            Err(error) if should_fallback_to_junction(&error) => {
                create_and_cache_junction(original, link)
            }
            result => result,
        },
        USE_JUNCTION => create_junction(original, link),
        _ => probe_and_cache(original, link),
    }
}

/// True symlinks on Windows take a relative target —
/// `path.relative(dirname(dest), src)`, the same form used on Unix.
/// Junctions take the absolute path with a trailing backslash, but
/// the `junction` crate handles that internally so we pass
/// `original` through unchanged for the junction branch.
fn create_true_symlink(original: &Path, link: &Path) -> io::Result<()> {
    let rel = super::relative_target_for(original, link);
    std::os::windows::fs::symlink_dir(&rel, link)
}

fn probe_and_cache(original: &Path, link: &Path) -> io::Result<()> {
    // Try the true directory symlink first — that's what users
    // running in Developer Mode (or as Administrator) get, and
    // true symlinks are preferred over junctions when allowed.
    // `CreateSymbolicLinkW` returns
    // `ERROR_PRIVILEGE_NOT_HELD` when the process can't create
    // symlinks; junctions don't carry that constraint, so fall
    // back to those.
    match create_true_symlink(original, link) {
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
        || matches!(
            error.raw_os_error(),
            Some(ERROR_DIRECTORY | ERROR_PRIVILEGE_NOT_HELD),
        )
}

fn create_and_cache_junction(original: &Path, link: &Path) -> io::Result<()> {
    let result = create_junction(original, link);
    if result.is_ok() {
        MODE.store(USE_JUNCTION, Ordering::Relaxed);
    }
    result
}

pub(super) fn create_junction(original: &Path, link: &Path) -> io::Result<()> {
    let staging = stage_junction(original, link)?;
    commit_staged_junction(link, &staging)
}

/// Build the junction at a unique sibling path so no other worker can
/// observe the `junction` crate's transient plain directory at `link`.
fn stage_junction(original: &Path, link: &Path) -> io::Result<PathBuf> {
    loop {
        let candidate = junction_staging_path(link);
        match junction::create(original, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

/// Publish `staging` at `link` with an atomic rename, folding a lost race
/// into the `AlreadyExists` reuse signal. A transient Windows file lock
/// on the rename is retried, one [`attempt_commit`] per try.
fn commit_staged_junction(link: &Path, staging: &Path) -> io::Result<()> {
    let rename_error = match super::retry_transient_file_locks(|| attempt_commit(link, staging)) {
        Ok(CommitAttempt::Committed) => return Ok(()),
        Ok(CommitAttempt::DestinationTaken) => {
            return Err(reuse_completed_destination(link, staging, ""));
        }
        Ok(CommitAttempt::InspectFailed(error)) => {
            return Err(inspect_failed(link, staging, &error, ""));
        }
        Err(error) => error,
    };

    // The rename either lost a cross-process race or genuinely failed;
    // re-inspecting the destination tells those apart.
    let after_rename = format!(" after a failed rename ({rename_error})");
    match inspect_destination(link) {
        Destination::Exists => Err(reuse_completed_destination(link, staging, &after_rename)),
        Destination::InspectFailed(error) => {
            Err(inspect_failed(link, staging, &error, &after_rename))
        }
        Destination::Missing => Err(discard_staging_after_rename(staging, link, rename_error)),
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
/// budget. Only the rename failure is returned as `Err`, so the retry
/// never repeats a final verdict about the destination.
fn attempt_commit(link: &Path, staging: &Path) -> io::Result<CommitAttempt> {
    let _commit_guard = JUNCTION_COMMIT_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    match inspect_destination(link) {
        Destination::Missing => {}
        Destination::Exists => return Ok(CommitAttempt::DestinationTaken),
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
            format!("failed to inspect junction destination {link:?}{context}: {inspect_error}",),
        ),
        Err(cleanup) => io::Error::other(format!(
            "failed to inspect junction destination {link:?}{context}: {inspect_error}; \
             staged-junction cleanup failed: {cleanup}",
        )),
    }
}

/// The rename genuinely failed and left nothing at `link`. Discard staging
/// and surface the rename error — but never as `AlreadyExists`, or
/// [`super::force_symlink_inner`] would treat the missing link as reusable.
/// A rename that lost the race only to have the winner vanish before the
/// re-inspection can carry that kind, so strip it to `Other`; every other
/// kind (including `NotFound`, which drives a mkdir + retry) is informative
/// and safe to surface unchanged.
pub(super) fn discard_staging_after_rename(
    staging: &Path,
    link: &Path,
    rename_error: io::Error,
) -> io::Error {
    match fs::remove_dir(staging) {
        Ok(()) if rename_error.kind() != io::ErrorKind::AlreadyExists => rename_error,
        Ok(()) => io::Error::other(format!(
            "failed to rename staged junction {staging:?} to {link:?}: {rename_error}",
        )),
        Err(cleanup) => io::Error::other(format!(
            "failed to rename staged junction {staging:?} to {link:?}: {rename_error}; \
             cleanup failed: {cleanup}",
        )),
    }
}

fn junction_staging_path(link: &Path) -> PathBuf {
    let id = JUNCTION_STAGING_ID.fetch_add(1, Ordering::Relaxed);
    let name = format!(".pnpm-junction-{}-{id}", std::process::id());
    link.with_file_name(name)
}
