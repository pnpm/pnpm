pub use placement::marker_present;

mod placement;

use placement::{all_files_match, file_matches_store_entry, populate_dir};

mod staging;
use staging::stage_and_swap;

use crate::{LinkFileError, remove_quarantine::remove_quarantine_from_native_binaries};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::PackageImportMethod;
use pnpm_reporter::Reporter;
use std::{
    collections::HashMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

/// Options for [`import_indexed_dir`].
///
/// Mirrors pnpm v11's `ImportOptions` at
/// `store/controller-types/src/index.ts` for the fields pacquet
/// consumes today. The defaults match the isolated linker's call
/// shape (no force, no nested-modules preservation); the hoisted
/// linker passes both flags set to `true`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImportIndexedDirOpts {
    /// When `true`, re-import even when `dir_path` already exists,
    /// overwriting the existing contents. Without `force`, an
    /// existing directory short-circuits this function (matches
    /// pnpm's pre-existence check in `importIndexedPackage`).
    pub force: bool,
    /// When `true` (only meaningful with `force`), preserve
    /// `dir_path/node_modules/` across the re-import so nested
    /// dependencies survive the rebuild. Required by the hoisted
    /// linker, whose orphan-removal and insert passes are
    /// interleaved across the package tree — a nested `node_modules/`
    /// installed by a sibling pass must not be clobbered when the
    /// parent package is re-imported.
    pub keep_modules_dir: bool,
    /// Whether an occupied, complete target is equivalent to this import.
    ///
    /// Callers must ensure that the target path uniquely identifies its contents.
    pub safe_to_skip: bool,
}

/// Error type for [`import_indexed_dir`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ImportIndexedDirError {
    #[display("cannot create directory at {dirname:?}: {error}")]
    CreateDir {
        dirname: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[diagnostic(transparent)]
    LinkFile(#[error(source)] LinkFileError),
    #[display("failed to inspect existing target {path:?}: {error}")]
    InspectTarget {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to clear non-directory dirent at {path:?}: {error}")]
    ClearNonDirEntry {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display(
        "failed to move existing {from:?} into staging directory {to:?} while preserving node_modules: {error}"
    )]
    PreserveModulesDir {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to remove existing directory {path:?} prior to swap: {error}")]
    RemoveExisting {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to rename staging directory {from:?} to {to:?}: {error}")]
    Swap {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to place imported file {from:?} at {to:?}: {error}")]
    PlaceFile {
        from: PathBuf,
        to: PathBuf,
        #[error(source)]
        error: io::Error,
    },
    #[display("failed to clear {path:?}, which blocks the repair of a partial import: {error}")]
    ClearBlockingDirEntry {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// How [`populate_dir`] puts each indexed entry at its final path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// Nothing of this import is at the target yet: link straight at the
    /// final path and adopt a dirent a concurrent importer placed first,
    /// since the two are importing the same content-addressed file.
    Fresh,
    /// The target already holds part of an import: replace whatever does
    /// not match the store entry, so a file damaged or truncated by an
    /// interrupted import is healed rather than adopted.
    Repair,
}

enum PreservedModules {
    None,
    Directory,
    Merged { backup: PathBuf, moved_entries: Vec<OsString> },
}

impl PreservedModules {
    fn has_moved_data(&self) -> bool {
        match self {
            PreservedModules::None => false,
            PreservedModules::Directory => true,
            PreservedModules::Merged { .. } => true,
        }
    }
}

struct PreserveModulesFailure {
    error: io::Error,
    preserved: PreservedModules,
}

impl Placement {
    /// How much an import into an occupied target may assume about what
    /// is already there, given whether the target is shared.
    fn for_target(safe_to_skip: bool) -> Self {
        if safe_to_skip { Placement::Repair } else { Placement::Fresh }
    }
}

/// Materialize an indexed package's files into `dir_path`, the way
/// pnpm v11's `importIndexedDir` does at
/// `fs/indexed-pkg-importer/src/importIndexedDir.ts`. The same function
/// services both node-linkers; behavior at the destination is
/// controlled by [`ImportIndexedDirOpts`].
///
/// Files in `cas_paths` are materialized by `import_into_fresh_target()`
/// using `import_method`'s preference order
/// (hardlink → reflink → copy, etc.), and the per-method
/// `pnpm:package-import-method` log is emitted via `logged_methods`
/// the first time each tier is used in this install. The pre-flight
/// `fs::metadata` short-circuit lives on `link_file()`; an import into a
/// private target skips it and relies on `import_into_fresh_target`'s
/// EEXIST tolerance, which is what keeps a marker-repair re-link over a
/// partial directory correct.
pub fn import_indexed_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    opts: ImportIndexedDirOpts,
) -> Result<(), ImportIndexedDirError> {
    let existing_kind = existing_dirent_kind(dir_path)?;

    // Drop the macOS quarantine xattr from the package's native binaries after
    // a populating import, matching pnpm's `removeQuarantineFromNativeBinaries`.
    // The marker-present short-circuit (and the non-directory dirent left as-is)
    // import nothing, so they skip the sweep, keeping warm installs free of the
    // per-install `xattr` cost — exactly pnpm's `!pkgExistsAtTargetDir` gate.
    let unquarantine = || remove_quarantine_from_native_binaries(dir_path, cas_paths);
    match (existing_kind, opts.force) {
        (None, _) => import_absent_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            opts.safe_to_skip,
        )
        .inspect(|()| unquarantine()),
        // Short-circuit only when the completion marker is present
        // (pnpm's `pkgExistsAtTargetDir`, which checks `package.json`),
        // not on mere directory existence. A marker-less directory is a
        // partial import; repair it by re-running the non-destructive
        // `populate_dir`. Ported from pnpm/pnpm#12204 (cbfeeef328).
        //
        // Whose partial import it is decides how much the repair may
        // assume: a private target holds this install's own interrupted
        // work, so an existing dirent is this package's file and is
        // adopted, while a shared one may hold a file an importer died
        // halfway through writing, which only a replacement heals.
        (Some(file_type), false) if file_type.is_dir() => repair_incomplete_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            opts.safe_to_skip,
        ),
        // A non-directory dirent is left as-is; only force=true clobbers it.
        (Some(_), false) => Ok(()),
        // Existing non-directory dirent with force=true. The hoisted
        // linker call shape won't produce this in practice, but
        // refusing to clobber a stale symlink would wedge the install.
        (Some(file_type), true) if !file_type.is_dir() => replace_non_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            file_type,
        )
        .inspect(|()| unquarantine()),
        // A forced refresh of a shared slot still works in place. Building a
        // complete stage first would duplicate every write before the rename
        // inevitably discovers that the shared directory already exists.
        (Some(file_type), true) if file_type.is_dir() && opts.safe_to_skip => {
            import_into_shared_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
                .inspect(|()| unquarantine())
        }
        (Some(_), true) => stage_and_swap::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            opts.keep_modules_dir,
        )
        .inspect(|()| unquarantine()),
    }
}

/// The kind of dirent already at `path`, or `None` when nothing is
/// there. Any inspection failure other than `NotFound` aborts the
/// import rather than being read as an absent target.
fn existing_dirent_kind(path: &Path) -> Result<Option<fs::FileType>, ImportIndexedDirError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta.file_type())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ImportIndexedDirError::InspectTarget { path: path.to_path_buf(), error }),
    }
}

/// Import into a target whose path is shared with the installs running
/// in other projects, without ever removing anything from it.
///
/// The importer that creates the directory populates it; the others heal
/// what is there, because a slot that exists is either finished, being
/// filled right now, or left behind by an importer that died mid-file —
/// and the three are indistinguishable from the outside.
fn import_into_shared_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
) -> Result<(), ImportIndexedDirError> {
    if claim_dir(dir_path)? {
        return populate_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            Placement::Fresh,
        );
    }
    if all_files_match(dir_path, cas_paths) {
        return Ok(());
    }
    populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths, Placement::Repair)
}

/// Create `dir_path`, reporting whether this call is the one that created
/// it. `create_dir_all` cannot answer that — it succeeds either way.
fn claim_dir(dir_path: &Path) -> Result<bool, ImportIndexedDirError> {
    if let Some(parent) = dir_path.parent() {
        fs::create_dir_all(parent).map_err(|error| ImportIndexedDirError::CreateDir {
            dirname: parent.to_path_buf(),
            error,
        })?;
    }
    match fs::create_dir(dir_path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => {
            Err(ImportIndexedDirError::CreateDir { dirname: dir_path.to_path_buf(), error })
        }
    }
}

// An absent shared target can be created concurrently; exclusive mkdir establishes ownership.
fn import_absent_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    safe_to_skip: bool,
) -> Result<(), ImportIndexedDirError> {
    if safe_to_skip {
        import_into_shared_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths)
    } else {
        populate_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            Placement::Fresh,
        )
    }
}

// A marker-less target is a partial import. Shared targets must replace potentially torn files.
fn repair_incomplete_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    safe_to_skip: bool,
) -> Result<(), ImportIndexedDirError> {
    if marker_present(dir_path, cas_paths) {
        Ok(())
    } else {
        populate_dir::<Reporter>(
            logged_methods,
            import_method,
            dir_path,
            cas_paths,
            Placement::for_target(safe_to_skip),
        )
        .inspect(|()| remove_quarantine_from_native_binaries(dir_path, cas_paths))
    }
}

fn replace_non_dir<Reporter: self::Reporter>(
    logged_methods: &AtomicU8,
    import_method: PackageImportMethod,
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    file_type: fs::FileType,
) -> Result<(), ImportIndexedDirError> {
    remove_non_dir_dirent(dir_path, file_type).map_err(|error| {
        ImportIndexedDirError::ClearNonDirEntry { path: dir_path.to_path_buf(), error }
    })?;
    populate_dir::<Reporter>(logged_methods, import_method, dir_path, cas_paths, Placement::Fresh)
}

/// Remove a non-directory dirent at `path`.
///
/// On Unix `fs::remove_file` unlinks any non-directory inode (regular
/// file, symlink-to-anywhere, fifo, socket). On Windows it rejects
/// directory symlinks and junctions — the OS treats those as
/// directory-shaped and they have to go through `remove_dir` instead.
/// Detect that case by resolving the link's target; if the target is
/// a directory (or the link is dangling but reports as a symlink),
/// route through `remove_dir`.
fn remove_non_dir_dirent(path: &Path, file_type: fs::FileType) -> io::Result<()> {
    #[cfg(windows)]
    if file_type.is_symlink() {
        // Resolved metadata follows the symlink: if the link points
        // at a directory (or is a junction, which Rust models as a
        // symlink whose target is a directory), `remove_dir` is the
        // correct call. Fall through to `remove_file` for dangling
        // links or symlinks-to-file.
        if matches!(fs::metadata(path), Ok(meta) if meta.is_dir()) {
            return fs::remove_dir(path);
        }
    }
    let _ = file_type;
    fs::remove_file(path)
}

#[cfg(test)]
mod tests;
