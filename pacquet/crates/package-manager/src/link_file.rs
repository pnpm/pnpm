use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::PackageImportMethod;
use pacquet_reporter::{
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

// Downgrade state machine used by both `Auto` and `CloneOrCopy`.
// These are the state *values*, not the cache itself: each mode keeps
// its own process-global `AtomicU8` (`AUTO_STATE` inside `link_file`,
// `CLONE_OR_COPY_STATE` likewise), so an `Auto` downgrade doesn't
// affect `CloneOrCopy` and vice versa.
//
// Neither cache is keyed by `(source fs, target fs)`. Once we observe
// a tier failing anywhere for a given mode, we stop trying it for the
// rest of the process. That's a coarse optimization to avoid paying
// the "try reflink, fail" cost for every file in installs where a
// higher tier is not usable on the store / workspace pair.
//
// A failure on one path can therefore downgrade later calls that
// would have succeeded on a different pair — in practice pacquet runs
// one install per process with one store and one target root, so this
// is fine. Pnpm's per-importer `let auto` closure (see
// `render-peer/fs/indexed-pkg-importer/src/index.ts`,
// `createAutoImporter` / `createCloneOrCopyImporter`) has the same
// coarseness once `pnpm install` has picked an import direction.
//
// The state is monotonic (`CLONE` → `HARDLINK` → `COPY`) and updated
// with `fetch_max`, so concurrent rayon workers racing on the first
// failure all converge to the same downgraded value without a lock.
// Worst case cost on startup is `N` stale attempts per tier where `N`
// is the rayon thread count — bounded, not per-file.
const LINK_STATE_CLONE: u8 = 0;
const LINK_STATE_HARDLINK: u8 = 1;
const LINK_STATE_COPY: u8 = 2;

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
    // Cutting the second `symlink_metadata` from the old shape —
    // it was a pure pessimization: in the clean-install case both
    // calls returned `NotFound`, doubling per-file `stat` count
    // (~260k extra syscalls on the alotta-files fixture). Defer
    // the dangling-symlink detection to the EEXIST recovery path
    // below, which only fires when the import call itself sees the
    // dirent.
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
/// EEXIST from the import syscall is still treated as a no-op:
/// concurrent installs (or a sibling rayon worker writing the same
/// CAFS path) can occasionally race past the freshness guarantee,
/// and the kernel's atomic-create error is the right way to detect
/// the contention. The content is content-addressed so the existing
/// dirent is equivalent.
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
    match try_import::<Reporter>(method, logged, source_file, target_link) {
        Ok(()) => Ok(()),
        // Mirrors pnpm's `linkOrCopy`: on EEXIST, return without
        // touching disk. A concurrent writer beat us to the target;
        // contents are content-addressed so leaving theirs in place
        // is equivalent.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(LinkFileError::Import {
            from: source_file.to_path_buf(),
            to: target_link.to_path_buf(),
            error,
        }),
    }
}

/// Run the import syscall for the configured `method`. Surfaces
/// the raw `io::Error` so the caller can dispatch on
/// `ErrorKind::AlreadyExists` for the EEXIST recovery path.
fn try_import<Reporter: self::Reporter>(
    method: PackageImportMethod,
    logged: &AtomicU8,
    source_file: &Path,
    target_link: &Path,
) -> io::Result<()> {
    match method {
        PackageImportMethod::Auto => {
            static AUTO_STATE: AtomicU8 = AtomicU8::new(LINK_STATE_CLONE);
            auto_link::<Reporter>(logged, &AUTO_STATE, source_file, target_link)
        }
        // pnpm's explicit `hardlink` method uses `hardlinkPkg(linkOrCopy)`
        // which falls back to copy on `EXDEV` (cross-device link not
        // permitted) but propagates other errors. Match that: if the
        // user asks for hardlink and they've put their store on a
        // different device from `node_modules`, copy silently; anything
        // else (missing source, permission denied, ...) is a real error
        // and should surface. No caching — the `fs::hard_link` syscall
        // itself is already cheap; pnpm doesn't cache this path either.
        PackageImportMethod::Hardlink => match fs::hard_link(source_file, target_link) {
            Ok(()) => {
                log_method_once::<Reporter>(logged, LOG_FLAG_HARDLINK, WireImportMethod::Hardlink);
                Ok(())
            }
            Err(error) if is_cross_device(&error) => fs::copy(source_file, target_link)
                .inspect(|_| {
                    log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
                })
                .map(drop),
            Err(error) => Err(error),
        },
        PackageImportMethod::Clone => {
            reflink_copy::reflink(source_file, target_link).inspect(|()| {
                log_method_once::<Reporter>(logged, LOG_FLAG_CLONE, WireImportMethod::Clone);
            })
        }
        PackageImportMethod::CloneOrCopy => {
            static CLONE_OR_COPY_STATE: AtomicU8 = AtomicU8::new(LINK_STATE_CLONE);
            clone_or_copy_link::<Reporter>(logged, &CLONE_OR_COPY_STATE, source_file, target_link)
        }
        PackageImportMethod::Copy => fs::copy(source_file, target_link)
            .inspect(|_| {
                log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
            })
            .map(drop),
    }
}

/// EXDEV = "cross-device link not permitted". Linux / macOS / BSD all
/// use errno 18; Windows maps its equivalent `ERROR_NOT_SAME_DEVICE`
/// to raw OS error 17. pnpm detects this by checking
/// `err.message.startsWith('EXDEV: cross-device link not permitted')` —
/// we can be a little tighter by looking at the raw errno.
///
/// The `17` mapping must stay Windows-only: on Unix, raw 17 is
/// `EEXIST` (surfaces as `ErrorKind::AlreadyExists`), which means a
/// concurrent process created the target between our `fs::metadata`
/// short-circuit and the link / reflink call. Falling back to
/// `fs::copy` on that signal would overwrite the other process's
/// freshly-installed file.
fn is_cross_device(err: &io::Error) -> bool {
    #[cfg(unix)]
    return err.raw_os_error() == Some(18);
    #[cfg(windows)]
    return err.raw_os_error() == Some(17);
    #[cfg(not(any(unix, windows)))]
    return false;
}

/// Errors that indicate the call itself is malformed (missing source,
/// permission denied, target already exists) — propagate these from
/// the downgrade cache instead of advancing to the next tier. A
/// different tier won't fix an invalid call, and downgrading on a
/// one-off `NotFound` would permanently disable reflink / hardlink for
/// every other file in the install.
///
/// Everything else — including the grab-bag of errno / Windows codes
/// kernels use to signal "filesystem can't do this operation"
/// (`EOPNOTSUPP`, `ENOTTY`, `ENOSYS`, `ERROR_INVALID_FUNCTION`, ...) —
/// triggers the fallback. This is the same deny-list the `reflink-copy`
/// crate uses in its own `reflink_or_copy` fallback logic, so it's
/// battle-tested across the platform matrix. The allow-list flavour we
/// tried initially missed Windows's `ERROR_INVALID_FUNCTION` (raw OS
/// `1`, which Rust surfaces as `ErrorKind::InvalidInput`) for NTFS's
/// rejection of `FSCTL_DUPLICATE_EXTENTS_TO_FILE`, breaking Windows CI.
fn is_call_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied | io::ErrorKind::AlreadyExists,
    )
}

/// `Auto`'s clone → hardlink → copy chain, using `state` to skip tiers
/// that have already failed in this process. Factored out so tests can
/// pass their own `AtomicU8` and exercise the downgrade logic in
/// isolation — the production path uses a `static` declared inside
/// [`link_file`]. Only capability / cross-device style failures
/// downgrade the cached state; other errors propagate immediately so a
/// one-off `NotFound` on a single file doesn't permanently disable a
/// tier for the rest of the process.
fn auto_link<Reporter: self::Reporter>(
    logged: &AtomicU8,
    state: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<()> {
    loop {
        match state.load(Ordering::Relaxed) {
            LINK_STATE_CLONE => match reflink_copy::reflink(source, target) {
                Ok(()) => {
                    log_method_once::<Reporter>(logged, LOG_FLAG_CLONE, WireImportMethod::Clone);
                    return Ok(());
                }
                Err(err) if is_call_error(&err) => return Err(err),
                Err(_) => {
                    state.fetch_max(LINK_STATE_HARDLINK, Ordering::Relaxed);
                }
            },
            LINK_STATE_HARDLINK => match fs::hard_link(source, target) {
                Ok(()) => {
                    log_method_once::<Reporter>(
                        logged,
                        LOG_FLAG_HARDLINK,
                        WireImportMethod::Hardlink,
                    );
                    return Ok(());
                }
                Err(err) if is_call_error(&err) => return Err(err),
                Err(_) => {
                    state.fetch_max(LINK_STATE_COPY, Ordering::Relaxed);
                }
            },
            _ => {
                return fs::copy(source, target)
                    .inspect(|_| {
                        log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
                    })
                    .map(drop);
            }
        }
    }
}

/// `CloneOrCopy`'s clone → copy chain with the same per-process cache
/// as [`auto_link`]. Differs from `Auto` by skipping the hardlink tier
/// entirely — matches pnpm's `createCloneOrCopyImporter`, which on
/// first reflink failure reassigns its closure directly to `copyPkg`.
/// Same error-narrowing as `auto_link`: only capability failures
/// downgrade; real errors propagate.
fn clone_or_copy_link<Reporter: self::Reporter>(
    logged: &AtomicU8,
    state: &AtomicU8,
    source: &Path,
    target: &Path,
) -> io::Result<()> {
    loop {
        match state.load(Ordering::Relaxed) {
            LINK_STATE_CLONE => match reflink_copy::reflink(source, target) {
                Ok(()) => {
                    log_method_once::<Reporter>(logged, LOG_FLAG_CLONE, WireImportMethod::Clone);
                    return Ok(());
                }
                Err(err) if is_call_error(&err) => return Err(err),
                Err(_) => {
                    state.fetch_max(LINK_STATE_COPY, Ordering::Relaxed);
                }
            },
            _ => {
                return fs::copy(source, target)
                    .inspect(|_| {
                        log_method_once::<Reporter>(logged, LOG_FLAG_COPY, WireImportMethod::Copy);
                    })
                    .map(drop);
            }
        }
    }
}

#[cfg(test)]
mod tests;
