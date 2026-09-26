pub use pnpm_fs::FsHardLink;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::PackageImportMethod;
use pnpm_fs::{FsReflink, Host, is_cross_device};
use pnpm_reporter::{
    LogEvent, LogLevel, PackageImportMethod as WireImportMethod, PackageImportMethodLog, Reporter,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU8, Ordering},
};

/// Error type for [`link_file`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkFileError {
    // `link_file` now dispatches to copy / reflink / hardlink depending
    // on `PackageImportMethod`, so a "fail to create a link" message
    // would be misleading when the configured method is `Copy`. Using
    // pnpm's "import" terminology (see `createPackageImporter`) so the
    // message is accurate regardless of which tier actually ran.
    #[display("failed to import {from:?} to {to:?}: {error}")]
    Import {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

// npm keeps process-global fallback tiers in `try_import`. Python keeps an
// ImportState per source filesystem inside each environment, so temporary
// build environments cannot downgrade project or npm imports.
//
// The state only ever moves forward along the platform's ladder (see
// [`next_auto_tier`]), each step taken with a compare-exchange from
// the exact tier that failed, so concurrent rayon workers racing on
// the first failure all converge to the same downgraded value without
// a lock: the loser's exchange fails, it reloads, and it finds the
// ladder already advanced. Worst case cost on startup is `N` stale
// attempts per tier where `N` is the rayon thread count — bounded,
// not per-file.
const LINK_STATE_CLONE: u8 = 0;
const LINK_STATE_HARDLINK: u8 = 1;
const LINK_STATE_COPY: u8 = 2;

/// The tier `Auto` starts at, per platform — the head of
/// [`next_auto_tier`]'s ladder.
#[cfg(target_os = "linux")]
const AUTO_FIRST_TIER: u8 = LINK_STATE_HARDLINK;
#[cfg(not(target_os = "linux"))]
const AUTO_FIRST_TIER: u8 = LINK_STATE_CLONE;

/// The wire method `Auto` optimistically resolves to on this platform —
/// the ladder head as `pnpm:progress` reports it. Progress events are
/// emitted before per-file resolution settles, so this is what the
/// `imported` message's `method` field carries for `Auto` installs.
#[must_use]
pub fn auto_optimistic_wire_method() -> WireImportMethod {
    match AUTO_FIRST_TIER {
        LINK_STATE_HARDLINK => WireImportMethod::Hardlink,
        _ => WireImportMethod::Clone,
    }
}

/// The tier `Auto` falls to when `tier` fails for capability reasons.
///
/// Linux runs hardlink before clone. A reflink is not the cheap tier
/// there: it materializes a new inode and copies extent bookkeeping
/// inside the filesystem's metadata trees, where a hardlink is one
/// directory entry and an nlink bump — measured on the alotta-files
/// fixture (39k files, warm store, btrfs), the whole install is 0.48s
/// hardlinked against 0.85s cloned, with kernel time 3.1s against
/// 5.3s. On ext4 the two orders behave identically, since `FICLONE`
/// is unsupported and every ladder ends at the hardlink tier. The
/// cost hardlinks carry is shared inodes: a package that mutates its
/// own files at runtime reaches the store copy — the same exposure
/// every ext4 and Windows install runs with, guarded by
/// `verify-store-integrity`, not by the import tier.
///
/// macOS keeps clone-first: APFS `clonefile` is the platform's cheap
/// primitive.
///
/// The hardlink-first order is a pnpm 12 change, shipped behind the
/// major: the TypeScript CLI (pnpm 11) deliberately keeps clone-first,
/// because changing what the default materializes on disk is not a
/// point-release change. The two `Auto` implementations intentionally
/// diverge on this until pnpm 11 is retired.
fn next_auto_tier(tier: u8) -> u8 {
    #[cfg(target_os = "linux")]
    match tier {
        LINK_STATE_HARDLINK => LINK_STATE_CLONE,
        _ => LINK_STATE_COPY,
    }
    #[cfg(not(target_os = "linux"))]
    match tier {
        LINK_STATE_CLONE => LINK_STATE_HARDLINK,
        _ => LINK_STATE_COPY,
    }
}

/// Advance the downgrade cache past `from`, unless another worker
/// already has.
fn downgrade_auto_tier(state: &AtomicU8, from: u8) {
    let _ =
        state.compare_exchange(from, next_auto_tier(from), Ordering::Relaxed, Ordering::Relaxed);
}

// One-shot "we picked this import method" log, matching pnpm's
// `packageImportMethodLogger.debug({ method: 'clone' | 'hardlink' | 'copy' })`
// in `fs/indexed-pkg-importer/src/index.ts`. Emits once per install per
// method so a reader of the logs can tell which tier actually ran —
// crucial for verifying hardlinks are kicking in on CI runners where
// reflink isn't available.
//
// The bitfield atomic is install-scoped, threaded down from
// `Install::run`, mirroring upstream's per-importer closure capture:
// pnpm's `createIndexedPackageImporter` builds a fresh closure per
// install, so a second install that wires up `pnpm:package-import-method`
// emits afresh. A module-static here would suppress emits on every
// install after the first in the same process — fine for the one-shot
// CLI today but a footgun for tests and any future embedded use.
//
// Each method gets two emits the first time it's used in an install: a
// `tracing::info!` for human / diagnostic logs, and a
// `pnpm:package-import-method` reporter event for structured consumers
// (`@pnpm/cli.default-reporter` and friends). `fetch_or` returns the
// previous bitfield, so the first caller to set a given bit is the one
// that emits.
const LOG_FLAG_CLONE: u8 = 1 << 0;
const LOG_FLAG_HARDLINK: u8 = 1 << 1;
const LOG_FLAG_COPY: u8 = 1 << 2;

fn log_method_once<Reporter: self::Reporter>(
    logged: &AtomicU8,
    flag: u8,
    method: WireImportMethod,
) {
    if logged.fetch_or(flag, Ordering::Relaxed) & flag == 0 {
        let method_name = match method {
            WireImportMethod::Clone => "clone",
            WireImportMethod::Hardlink => "hardlink",
            WireImportMethod::Copy => "copy",
        };
        tracing::info!(target: "pacquet::package_import_method", method = method_name, "selected package import method");
        Reporter::emit(&LogEvent::PackageImportMethod(PackageImportMethodLog {
            level: LogLevel::Debug,
            method,
        }));
    }
}

/// Materialize a CAFS file into `target_link` using `method`.
///
/// * If `target_link` already exists, do nothing.
/// * `target_link.parent()` must already exist; this is a leaf
///   operation that does not create directories. Mirrors pnpm v11's
///   `importFile` (see `fs/indexed-pkg-importer/src/importIndexedDir.ts`,
///   `tryImportIndexedDir`), which mkdirs the unique parent set
///   sequentially up-front and then calls into the import primitive
///   per file. [`import_indexed_dir`](crate::import_indexed_dir()) is
///   the production caller and handles that pre-pass.
pub fn link_file<Reporter: self::Reporter>(
    logged: &AtomicU8,
    method: PackageImportMethod,
    source_file: &Path,
    target_link: &Path,
) -> Result<(), LinkFileError> {
    // Single `stat` short-circuit. If the target resolves to a live
    // file (directly or via a symlink), a prior install placed it
    // and there's nothing to do — return without paying for the
    // import syscall (which would overwrite on the `Copy` /
    // `Auto`-fallback-to-copy path, mismatching the no-op contract
    // the test suite locks in).
    //
    // A single `metadata` stat suffices: dangling-symlink detection
    // is deferred to the EEXIST recovery path below, which only fires
    // when the import call itself sees the dirent. A second
    // `symlink_metadata` here would double the per-file stat count in
    // the clean-install case (both calls return `NotFound`).
    //
    // For `NotFound` and any other stat error, fall through to the
    // import call — it will surface the real error or succeed.
    if fs::metadata(target_link).is_ok() {
        return Ok(());
    }

    import_into_fresh_target::<Reporter>(logged, method, source_file, target_link)
}

/// Same as [`link_file`] but without the pre-flight `fs::metadata`
/// stat. Caller guarantees `target_link` is fresh (does not currently
/// exist) — the import syscall is invoked directly.
///
/// On the alotta-files fixture this saves ~170k `stat` syscalls per
/// clean install. The pre-flight stat in [`link_file`] only matters
/// to preserve the no-op-on-existing-target contract for the
/// `Copy` / downgraded `Auto`→`Copy` / `CloneOrCopy`→`Copy` paths,
/// where `fs::copy` would otherwise silently overwrite. When the
/// caller knows the target is fresh — as is the case for
/// `crate::import_indexed_dir::populate_dir`, which only ever
/// runs against a directory it just created — that protection is
/// unneeded.
///
/// A concurrent install (or a sibling rayon worker writing the same
/// CAFS path) can race past the freshness guarantee;
/// `recover_from_concurrent_import` decides which import failures mean
/// that and adopts the file it placed.
pub fn import_into_fresh_target<Reporter: self::Reporter>(
    logged: &AtomicU8,
    method: PackageImportMethod,
    source_file: &Path,
    target_link: &Path,
) -> Result<(), LinkFileError> {
    // Hardlinking a file from the store into `node_modules` means any
    // package that edits its own files at runtime (postinstall scripts
    // are the usual offender) ends up mutating the shared store copy.
    // Current pnpm's indexed-pkg-importer does not guard against this
    // either — postinstall handling lives in the script runner, not the
    // import layer — so there's nothing to gate on here.
    try_import::<Reporter, Host>(method, logged, source_file, target_link)
        .map(|_| ())
        .or_else(|error| recover_from_concurrent_import(error, source_file, target_link))
}

/// Resolve an import syscall failure against a target the caller
/// believed fresh.
///
/// `AlreadyExists` means a concurrent writer beat us to the target; its
/// content is content-addressed and equivalent. pnpm's `linkOrCopy`
/// returns here without touching disk, but a reflinked or copied target
/// carries whichever mode its writer's umask gave it, so the adopted
/// dirent is aligned with the current umask's store-entry mode first.
/// That alignment also heals a target an earlier failure left with the
/// wrong mode; the clone tier relies on it and keeps such a target in
/// place.
///
/// `NotFound` is the same race when a regular file now sits at the
/// target: APFS `clonefile` intermittently reports a destination that
/// another process renamed into place moments earlier as missing instead
/// of existing (pnpm/pnpm#14560). Both checks follow symlinks, so a
/// dangling link squatting at either path is not mistaken for the race:
/// a store blob that really is gone stays an error, and so does a target
/// the copy tier could not open through.
///
/// A symlink squatting at the target is left exactly as pnpm leaves it:
/// no writer materialized it, and the alignment would only fail on it.
/// Every other error is the caller's to surface.
fn is_placed_concurrently(error: &io::Error, source_file: &Path, target_link: &Path) -> bool {
    error.kind() == io::ErrorKind::AlreadyExists
        || (error.kind() == io::ErrorKind::NotFound
            && fs::metadata(target_link).is_ok_and(|m| m.is_file())
            && fs::metadata(source_file).is_ok())
}

/// Materialize an independent copy of `source_file` over `target_link`
/// carrying its [`desired_permissions`], leaving `source_file`
/// untouched.
fn replace_shared_inode_with_copy(source_file: &Path, target_link: &Path) -> io::Result<()> {
    pnpm_fs::copy_file_atomic_with_permissions(
        source_file,
        target_link,
        &desired_permissions(source_file)?,
    )
}

fn recover_from_concurrent_import(
    error: io::Error,
    source_file: &Path,
    target_link: &Path,
) -> Result<(), LinkFileError> {
    let import_error = |error| LinkFileError::Import {
        from: source_file.to_path_buf(),
        to: target_link.to_path_buf(),
        error,
    };
    if !is_placed_concurrently(&error, source_file, target_link) {
        return Err(import_error(error));
    }
    if fs::symlink_metadata(target_link).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Ok(());
    }
    if same_inode(source_file, target_link) {
        if source_has_desired_mode(source_file).map_err(import_error)? {
            return Ok(());
        }
        return replace_shared_inode_with_copy(source_file, target_link).map_err(import_error);
    }
    match align_target_mode(source_file, target_link) {
        Ok(()) => Ok(()),
        // Nothing serializes shared-slot imports across
        // processes: the writer that owns the target may
        // replace it again before this chmod lands. Whoever
        // unlinked the path writes an equivalent
        // content-addressed file and restores its exec bit in
        // turn, so `NotFound` with the dirent actually gone
        // means another writer finished the job — the same
        // tolerance the bin-shim chmod applies
        // (`chmod_tolerating_removal` in `pnpm-cmd-shim`,
        // pnpm/pnpm#14353).
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && matches!(
                    fs::symlink_metadata(target_link),
                    Err(ref stat_error) if stat_error.kind() == io::ErrorKind::NotFound,
                ) =>
        {
            Ok(())
        }
        Err(error) => Err(import_error(error)),
    }
}

/// Cached fallback tiers for one source/target filesystem pair.
pub struct ImportState {
    auto: AtomicU8,
    clone_or_copy: AtomicU8,
}

impl ImportState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            auto: AtomicU8::new(AUTO_FIRST_TIER),
            clone_or_copy: AtomicU8::new(LINK_STATE_CLONE),
        }
    }

    /// Import a file using the configured method and return the method actually used.
    /// Existing destinations and missing sources are errors; no existing file is adopted.
    pub fn import<Reporter: self::Reporter, Sys: FsHardLink + FsReflink>(
        &self,
        method: PackageImportMethod,
        logged: &AtomicU8,
        source_file: &Path,
        target_link: &Path,
    ) -> io::Result<WireImportMethod> {
        match method {
            PackageImportMethod::Auto => {
                auto_link::<Reporter, Sys>(logged, &self.auto, source_file, target_link)
            }
            PackageImportMethod::Hardlink => {
                hardlink_file::<Reporter, Sys>(logged, source_file, target_link)
            }
            PackageImportMethod::Clone => clone_file::<Sys>(source_file, target_link)
                .inspect(|()| {
                    log_method_once::<Reporter>(logged, LOG_FLAG_CLONE, WireImportMethod::Clone);
                })
                .map(|()| WireImportMethod::Clone),
            PackageImportMethod::CloneOrCopy => clone_or_copy_link::<Reporter, Sys>(
                logged,
                &self.clone_or_copy,
                source_file,
                target_link,
            ),
            PackageImportMethod::Copy => copy_file(source_file, target_link)
                .inspect(|()| {
                    log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
                })
                .map(|()| WireImportMethod::Copy),
        }
    }
}

impl Default for ImportState {
    fn default() -> Self {
        Self::new()
    }
}

fn hardlink_file<Reporter: self::Reporter, Sys: FsHardLink>(
    logged: &AtomicU8,
    source_file: &Path,
    target_link: &Path,
) -> io::Result<WireImportMethod> {
    // pnpm's explicit `hardlink` method uses `hardlinkPkg(linkOrCopy)`,
    // which copies on any link failure other than `EEXIST`. Only
    // `EXDEV` copies here: a store on a different device from
    // `node_modules` is a placement the user can change, and one
    // package's copy is cheap. A source that has run out of names
    // ([`is_too_many_links`]) copies for the same reason: it costs
    // one file, not the install. Everything else surfaces, `EPERM`
    // included — a filesystem that refuses links would copy every
    // package, which is the disk cost `hardlink` was chosen to
    // avoid, so the user gets an error naming the method instead of
    // a silent whole-install copy. No caching — the `fs::hard_link`
    // syscall itself is already cheap; pnpm doesn't cache this path
    // either.
    //
    // A mode that does not match the current umask cannot be fixed on the
    // store inode every name shares, so the file is copied at the
    // [`desired_mode`] instead (pnpm/pnpm#3807).
    if source_has_desired_mode(source_file)? {
        match Sys::hard_link(source_file, target_link) {
            Ok(()) => {
                log_method_once::<Reporter>(logged, LOG_FLAG_HARDLINK, WireImportMethod::Hardlink);
                Ok(WireImportMethod::Hardlink)
            }
            Err(error) if is_cross_device(&error) || is_too_many_links(&error) => {
                copy_file(source_file, target_link)
                    .inspect(|()| {
                        log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
                    })
                    .map(|()| WireImportMethod::Copy)
            }
            Err(error) => Err(error),
        }
    } else {
        copy_file(source_file, target_link)
            .inspect(|()| {
                log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
            })
            .map(|()| WireImportMethod::Copy)
    }
}

fn try_import<Reporter: self::Reporter, Sys: FsHardLink + FsReflink>(
    method: PackageImportMethod,
    logged: &AtomicU8,
    source_file: &Path,
    target_link: &Path,
) -> io::Result<WireImportMethod> {
    static STATE: ImportState = ImportState::new();
    STATE.import::<Reporter, Sys>(method, logged, source_file, target_link)
}

/// Materialize `source_file` at `target_link` for the copy tier, at the
/// mode a fresh store write under the current umask would give the CAS
/// entry rather than the source's population-time mode (pnpm/pnpm#3807).
///
/// [`pnpm_fs::copy_file_exclusive`] creates the target exclusively, so
/// an occupied target raises `AlreadyExists` and reaches
/// [`recover_from_concurrent_import`], where every tier sends one. A
/// partial file it removes on failure would otherwise be adopted by a
/// later import as a concurrent writer's finished work.
fn desired_permissions(source_file: &Path) -> io::Result<fs::Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::Permissions::from_mode(desired_mode(source_file)))
    }
    #[cfg(not(unix))]
    fs::File::open(source_file)?.metadata().map(|meta| meta.permissions())
}

fn copy_file(source_file: &Path, target_link: &Path) -> io::Result<()> {
    pnpm_fs::copy_file_exclusive(
        source_file,
        target_link,
        &desired_permissions(source_file)?,
        |_| Ok(()),
    )
}

/// [`FsReflink::reflink`] for the explicit `Clone` method, then
/// alignment with the store-entry mode.
fn clone_file<Sys: FsReflink>(source_file: &Path, target_link: &Path) -> io::Result<()> {
    Sys::reflink(source_file, target_link)?;
    align_target_mode(source_file, target_link)
}

/// The mode the materialized file should carry: what a fresh store
/// write under the current process umask would give this CAS entry
/// (pnpm/pnpm#3807).
#[cfg(unix)]
fn desired_mode(source_file: &Path) -> u32 {
    pnpm_fs::file_mode::store_entry_mode(
        pnpm_fs::file_mode::cas_path_is_executable(source_file),
        pnpm_fs::file_mode::current_umask(),
    )
}

/// Whether `source_file`'s on-disk mode is [`desired_mode`]. A mode that
/// differs cannot be fixed on a store inode, so the hardlink tiers copy
/// instead; a reflink that copied the same mode onto the target is
/// aligned instead.
#[cfg(unix)]
fn source_has_desired_mode(source_file: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(source_file)?.mode() & 0o777 == desired_mode(source_file))
}

/// Align a materialized `target_link` with the mode a fresh store write
/// under the current umask would give `source_file`. A reflink carries the
/// source's mode onto the target — `clonefile` copies its attributes and the
/// reflink tier sets them explicitly — so an entry the store wrote under a
/// wider umask keeps that wider mode in `node_modules` until it is aligned
/// here (pnpm/pnpm#3807).
#[cfg(unix)]
fn align_target_mode(source_file: &Path, target_link: &Path) -> io::Result<()> {
    pnpm_fs::file_mode::set_path_permissions(target_link, desired_mode(source_file))
}

#[cfg(not(unix))]
fn source_has_desired_mode(_source_file: &Path) -> io::Result<bool> {
    Ok(true)
}

#[cfg(not(unix))]
fn align_target_mode(_source_file: &Path, _target_link: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn same_inode(source: &Path, target: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    matches!(
        (fs::metadata(source), fs::metadata(target)),
        (Ok(source_meta), Ok(target_meta))
            if source_meta.ino() == target_meta.ino() && source_meta.dev() == target_meta.dev(),
    )
}

#[cfg(not(unix))]
fn same_inode(_source: &Path, _target: &Path) -> bool {
    false
}

/// Unix permission errors that may deny linking while still allowing copying.
/// Android's `SELinux` policy can reject hardlinks with `EACCES`; filesystems
/// without link support and restricted containers can return `EPERM`.
/// The copy tier reports any remaining access error on the paths.
fn is_link_permission_error(err: &io::Error) -> bool {
    #[cfg(unix)]
    return matches!(err.raw_os_error(), Some(libc::EPERM | libc::EACCES));
    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
}

/// The source file already carries every name the filesystem will give
/// it: 1024 on NTFS, 65000 on ext4. Unlike [`is_cross_device`] and
/// [`is_link_permission_error`], this is a property of one file
/// rather than of the filesystem, so it must not retire a tier — every
/// other file in the install can still be hardlinked, and only this one
/// has to be materialized another way. Copying is the only thing that
/// helps, and it is what pnpm's `linkOrCopy` does here.
///
/// `std` maps `EMLINK` and `ERROR_TOO_MANY_LINKS` to the same kind, so
/// unlike its peers this one needs no raw code.
fn is_too_many_links(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::TooManyLinks
}

/// Errors that must propagate without advancing the cached import tier.
/// Missing paths and existing targets cannot be fixed by changing methods.
/// Unix link permission errors permit fallback; see [`is_link_permission_error`].
/// Other permission errors remain terminal.
///
/// All other errors allow fallback, including Windows's `ERROR_INVALID_FUNCTION`
/// (`InvalidInput`) when NTFS rejects `FSCTL_DUPLICATE_EXTENTS_TO_FILE`.
fn is_call_error(err: &io::Error) -> bool {
    match err.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::AlreadyExists => true,
        io::ErrorKind::PermissionDenied => !is_link_permission_error(err),
        _ => false,
    }
}

/// `Auto`'s downgrade chain — hardlink → clone → copy on Linux,
/// clone → hardlink → copy elsewhere (see [`next_auto_tier`] for the
/// why) — using `state` to skip tiers that have already failed in this
/// process. Factored out so tests can
/// pass their own `AtomicU8` and exercise the downgrade logic in
/// isolation — the production path uses a `static` declared inside
/// [`link_file`]. Only capability / cross-device style failures
/// downgrade the cached state; other errors propagate immediately so a
/// one-off `NotFound` on a single file doesn't permanently disable a
/// tier for the rest of the process.
fn copy_and_log<Reporter: self::Reporter>(
    logged: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<WireImportMethod> {
    copy_file(source, target)?;
    log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
    Ok(WireImportMethod::Copy)
}

fn auto_link<Reporter: self::Reporter, Sys: FsHardLink + FsReflink>(
    logged: &AtomicU8,
    state: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<WireImportMethod> {
    loop {
        match state.load(Ordering::Relaxed) {
            LINK_STATE_CLONE => {
                if clone_tier::<Reporter, Sys>(logged, source, target)? {
                    return Ok(WireImportMethod::Clone);
                }
                downgrade_auto_tier(state, LINK_STATE_CLONE);
            }
            LINK_STATE_HARDLINK => {
                if let Some(method) = hardlink_tier::<Reporter, Sys>(logged, source, target)? {
                    return Ok(method);
                }
                downgrade_auto_tier(state, LINK_STATE_HARDLINK);
            }
            _ => return copy_and_log::<Reporter>(logged, source, target),
        }
    }
}

/// Reflink `source` to `target`. `Ok(false)` means the tier is unusable
/// on this filesystem pair and the caller should downgrade to the next
/// one.
///
/// Only the reflink itself may downgrade. Alignment runs after
/// reflink created the target, so its error is terminal — downgrading
/// on it would re-attempt the next tier against that just-created file
/// and mask the real error behind `AlreadyExists`.
fn clone_tier<Reporter: self::Reporter, Sys: FsReflink>(
    logged: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<bool> {
    match Sys::reflink(source, target) {
        Ok(()) => {
            align_target_mode(source, target)?;
            log_method_once::<Reporter>(logged, LOG_FLAG_CLONE, WireImportMethod::Clone);
            Ok(true)
        }
        Err(err) if is_call_error(&err) => Err(err),
        Err(_) => Ok(false),
    }
}

/// Hardlink `source` to `target`, with the same downgrade contract as
/// [`clone_tier`], plus one outcome [`clone_tier`] has no equivalent
/// for: a source out of names copies here and reports the tier still
/// usable, because [`is_too_many_links`] says nothing about the next
/// file.
/// A source whose on-disk mode does not match the current umask's
/// store-entry mode copies at the right mode, keeping the tier for the
/// files whose mode does match (pnpm/pnpm#3807).
fn hardlink_tier<Reporter: self::Reporter, Sys: FsHardLink>(
    logged: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<Option<WireImportMethod>> {
    if source_has_desired_mode(source)? {
        match Sys::hard_link(source, target) {
            Ok(()) => {
                log_method_once::<Reporter>(logged, LOG_FLAG_HARDLINK, WireImportMethod::Hardlink);
                Ok(Some(WireImportMethod::Hardlink))
            }
            Err(err) if is_too_many_links(&err) => {
                copy_and_log::<Reporter>(logged, source, target).map(Some)
            }
            Err(err) if is_call_error(&err) => Err(err),
            Err(_) => Ok(None),
        }
    } else {
        copy_and_log::<Reporter>(logged, source, target).map(Some)
    }
}

/// `CloneOrCopy`'s clone → copy chain with the same per-process cache
/// as [`auto_link`]. Differs from `Auto` by skipping the hardlink tier
/// entirely — matches pnpm's `createCloneOrCopyImporter`, which on
/// first reflink failure reassigns its closure directly to `copyPkg`.
/// Same error-narrowing as [`auto_link`]: only capability failures
/// downgrade; real errors propagate.
fn clone_or_copy_link<Reporter: self::Reporter, Sys: FsReflink>(
    logged: &AtomicU8,
    state: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<WireImportMethod> {
    loop {
        match state.load(Ordering::Relaxed) {
            LINK_STATE_CLONE => {
                if clone_tier::<Reporter, Sys>(logged, source, target)? {
                    return Ok(WireImportMethod::Clone);
                }
                state.fetch_max(LINK_STATE_COPY, Ordering::Relaxed);
            }
            _ => return copy_and_log::<Reporter>(logged, source, target),
        }
    }
}

#[cfg(test)]
mod tests;
