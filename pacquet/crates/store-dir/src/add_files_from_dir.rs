//! Re-CAFS an already-extracted package directory: walks the tree,
//! writes each file into the content-addressed store, and returns
//! the resulting `path → file metadata` map.
//!
//! Ports pnpm's
//! [`addFilesFromDir`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/store/cafs/src/addFilesFromDir.ts)
//! and the surrounding worker-side glue at
//! [`worker/src/start.ts:312-383`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/worker/src/start.ts#L312-L383).
//! Used by the side-effects-cache WRITE path: after a postinstall
//! script modifies the package directory, this function rehashes
//! the directory so [`upload`](crate::upload()) can diff it against
//! the pristine `PackageFilesIndex.files` row and seed the cache.

use crate::{CafsFileInfo, StoreDir, WriteCasFileError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::file_mode::is_executable;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

/// Result of [`add_files_from_dir()`]. The map's key is the file's
/// path *relative to `pkg_root`*, with forward-slash separators —
/// matching upstream's `${relativeDir}/${file.name}` shape so the
/// resulting `FilesIndex` round-trips through pnpm without
/// renormalisation.
#[derive(Debug)]
pub struct AddedFiles {
    pub files: HashMap<String, CafsFileInfo>,
}

/// Error type of [`add_files_from_dir()`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AddFilesFromDirError {
    #[display("Failed to canonicalize package root {}: {source}", root.display())]
    CanonicalizeRoot {
        root: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to read directory {}: {source}", dir.display())]
    ReadDir {
        dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to stat {}: {source}", path.display())]
    Stat {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("Failed to read file {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[diagnostic(transparent)]
    WriteCas(#[error(source)] WriteCasFileError),
}

/// Walk `pkg_root` and write every regular file into `store_dir`'s
/// CAFS, producing an `AddedFiles { files }` map. Symlinks are
/// followed only when they resolve inside `pkg_root` (mirrors
/// upstream's `isSubdir(rootDir, realPath)` containment check at
/// [`addFilesFromDir.ts:112`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/store/cafs/src/addFilesFromDir.ts#L112));
/// out-of-root targets are silently skipped. A top-level
/// `node_modules` directory is skipped unconditionally (upstream's
/// `includeNodeModules` defaults to `false` and pacquet has no
/// caller that flips it).
///
/// Cycle-safe: `WalkCtx.visited` is a *recursion-stack* set —
/// each canonical directory path is inserted when we descend
/// into it and removed when we return. A symlink pointing back
/// at an ancestor of the current branch finds the ancestor's
/// canonical path in `visited` and bails. Mirrors upstream's
/// [`ctx.visited`](https://github.com/pnpm/pnpm/blob/7e3145f9fc/store/cafs/src/addFilesFromDir.ts#L121-L173)
/// semantics: same directory can be visited twice if reached
/// through two distinct paths (e.g. a shared subgraph), but
/// cycles still terminate because the path-to-root is never
/// re-entered.
pub fn add_files_from_dir(
    store_dir: &StoreDir,
    pkg_root: &Path,
) -> Result<AddedFiles, AddFilesFromDirError> {
    let canonical_root = dunce::canonicalize(pkg_root).map_err(|source| {
        AddFilesFromDirError::CanonicalizeRoot { root: pkg_root.to_path_buf(), source }
    })?;
    let mut ctx = WalkCtx {
        files: HashMap::new(),
        canonical_root: canonical_root.clone(),
        visited: HashSet::from([canonical_root.clone()]),
        store_dir,
    };
    walk(&mut ctx, pkg_root, "", &canonical_root)?;
    Ok(AddedFiles { files: ctx.files })
}

struct WalkCtx<'a> {
    files: HashMap<String, CafsFileInfo>,
    canonical_root: PathBuf,
    visited: HashSet<PathBuf>,
    store_dir: &'a StoreDir,
}

fn walk(
    ctx: &mut WalkCtx<'_>,
    dir: &Path,
    relative_dir: &str,
    current_real_path: &Path,
) -> Result<(), AddFilesFromDirError> {
    let entries = fs::read_dir(dir)
        .map_err(|source| AddFilesFromDirError::ReadDir { dir: dir.to_path_buf(), source })?;
    for entry in entries {
        let entry = entry
            .map_err(|source| AddFilesFromDirError::ReadDir { dir: dir.to_path_buf(), source })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let relative_subpath = if relative_dir.is_empty() {
            name.to_string()
        } else {
            format!("{relative_dir}/{name}")
        };
        let absolute = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| AddFilesFromDirError::Stat { path: absolute.clone(), source })?;

        let mut next_real_dir: Option<PathBuf> = None;
        // Path to use for the read/stat after this branch:
        // - regular file or directory: the original entry path.
        // - symlink to a file: the *resolved* target path. Reading
        //   from the resolved path closes a TOCTOU where the
        //   symlink could be retargeted between the containment
        //   check and the read, otherwise letting us ingest data
        //   from outside `pkg_root`.
        let mut read_path = absolute.clone();
        let mut symlink_target_meta: Option<fs::Metadata> = None;

        if file_type.is_symlink() {
            // Upstream's `getSymlinkStatIfContained`: realpath the
            // symlink and validate it resolves inside the package
            // root. Broken or out-of-root symlinks are skipped
            // silently.
            let real = match dunce::canonicalize(&absolute) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !real.starts_with(&ctx.canonical_root) {
                continue;
            }
            let meta = fs::metadata(&real)
                .map_err(|source| AddFilesFromDirError::Stat { path: real.clone(), source })?;
            if meta.is_dir() {
                next_real_dir = Some(real);
            } else {
                symlink_target_meta = Some(meta);
                read_path = real;
            }
        } else if file_type.is_dir() {
            next_real_dir = Some(current_real_path.join(&*name));
        }

        if let Some(real_dir) = next_real_dir {
            if ctx.visited.contains(&real_dir) {
                continue;
            }
            // Mirror upstream's top-level-only `node_modules` skip
            // (`relativeDir !== '' || file.name !== 'node_modules'`).
            // Pacquet's `includeNodeModules` is implicitly `false`
            // because no caller has needed it yet.
            if relative_dir.is_empty() && name == "node_modules" {
                continue;
            }
            ctx.visited.insert(real_dir.clone());
            // Recurse via the resolved directory so a symlinked
            // sub-directory's contents are walked from the canonical
            // path. Matches the TOCTOU rationale above for file
            // reads.
            walk(ctx, &real_dir, &relative_subpath, &real_dir)?;
            ctx.visited.remove(&real_dir);
            continue;
        }

        let meta = match symlink_target_meta {
            Some(m) => m,
            None => fs::metadata(&read_path)
                .map_err(|source| AddFilesFromDirError::Stat { path: read_path.clone(), source })?,
        };
        if !meta.is_file() {
            continue;
        }
        let buffer = fs::read(&read_path)
            .map_err(|source| AddFilesFromDirError::ReadFile { path: read_path.clone(), source })?;
        let mode = file_mode_from(&meta);
        let executable = is_executable(mode);
        let (_path, hash) = ctx
            .store_dir
            .write_cas_file(&buffer, executable)
            .map_err(AddFilesFromDirError::WriteCas)?;
        ctx.files.insert(
            relative_subpath,
            CafsFileInfo {
                digest: format!("{hash:x}"),
                mode,
                size: buffer.len() as u64,
                checked_at: None,
            },
        );
    }
    Ok(())
}

/// Return the file mode bits in pnpm's canonical form.
/// On Unix this is `metadata.mode() & 0o777`; on Windows there is
/// no analog so a fixed `0o644` is reported (matches what pnpm
/// itself writes for tarball entries on Windows hosts).
#[cfg(unix)]
fn file_mode_from(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn file_mode_from(_meta: &fs::Metadata) -> u32 {
    0o644
}

#[cfg(test)]
mod tests;
