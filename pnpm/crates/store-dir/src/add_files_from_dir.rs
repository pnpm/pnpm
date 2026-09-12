//! Re-CAFS an already-extracted package directory: walks the tree,
//! writes each file into the content-addressed store, and returns
//! the resulting `path → file metadata` map.
//!
//! Used by the side-effects-cache WRITE path: after a postinstall
//! script modifies the package directory, this function rehashes
//! the directory so [`upload`](crate::upload()) can diff it against
//! the pristine `PackageFilesIndex.files` row and seed the cache.

use crate::{CafsFileInfo, StoreDir, WriteCasFileError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::file_mode::is_executable;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

/// Result of [`add_files_from_dir()`]. The map's key is the file's
/// path *relative to `pkg_root`*, with forward-slash separators, so the
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
/// CAFS, producing an `AddedFiles { files }` map.
///
/// Cycle-safe: `WalkCtx.visited` is a *recursion-stack* set —
/// each canonical directory path is inserted when we descend
/// into it and removed when we return. A symlink pointing back
/// at an ancestor of the current branch finds the ancestor's
/// canonical path in `visited` and bails. The same directory can
/// still be visited twice if reached through two distinct paths
/// (e.g. a shared subgraph), because only the path-to-root is
/// guarded.
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

        let Some(target) = resolve_entry(ctx, &entry, current_real_path, &name)? else {
            continue;
        };
        match target {
            EntryTarget::Directory(real_dir) => {
                walk_subdirectory(ctx, &real_dir, relative_dir, &name, &relative_subpath)?;
            }
            EntryTarget::File { read_path, meta } => {
                ingest_file(ctx, &read_path, meta.as_ref(), relative_subpath)?;
            }
        }
    }
    Ok(())
}

/// What an entry resolves to, or `None` when it points outside the package
/// root and is skipped.
enum EntryTarget {
    Directory(PathBuf),
    /// Path to read the payload from, and the target's metadata when
    /// resolving the entry already stat'ed it.
    ///
    /// For a symlink this is the *resolved* target path. Reading from the
    /// resolved path closes a TOCTOU where the symlink could be retargeted
    /// between the containment check and the read, otherwise letting us
    /// ingest data from outside `pkg_root`.
    File {
        read_path: PathBuf,
        meta: Option<fs::Metadata>,
    },
}

fn resolve_entry(
    ctx: &WalkCtx<'_>,
    entry: &fs::DirEntry,
    current_real_path: &Path,
    name: &str,
) -> Result<Option<EntryTarget>, AddFilesFromDirError> {
    let absolute = entry.path();
    let file_type = entry
        .file_type()
        .map_err(|source| AddFilesFromDirError::Stat { path: absolute.clone(), source })?;

    if file_type.is_dir() {
        return Ok(Some(EntryTarget::Directory(current_real_path.join(name))));
    }
    if !file_type.is_symlink() {
        return Ok(Some(EntryTarget::File { read_path: absolute, meta: None }));
    }

    let Ok(real) = dunce::canonicalize(&absolute) else { return Ok(None) };
    if !real.starts_with(&ctx.canonical_root) {
        return Ok(None);
    }
    let meta = fs::metadata(&real)
        .map_err(|source| AddFilesFromDirError::Stat { path: real.clone(), source })?;
    if meta.is_dir() {
        return Ok(Some(EntryTarget::Directory(real)));
    }
    Ok(Some(EntryTarget::File { read_path: real, meta: Some(meta) }))
}

/// Recurse via the resolved directory so a symlinked sub-directory's
/// contents are walked from the canonical path, matching the TOCTOU
/// rationale for file reads. A directory already on the walk's path is a
/// symlink cycle and is skipped.
fn walk_subdirectory(
    ctx: &mut WalkCtx<'_>,
    real_dir: &Path,
    relative_dir: &str,
    name: &str,
    relative_subpath: &str,
) -> Result<(), AddFilesFromDirError> {
    if ctx.visited.contains(real_dir) || (relative_dir.is_empty() && name == "node_modules") {
        return Ok(());
    }
    ctx.visited.insert(real_dir.to_path_buf());
    walk(ctx, real_dir, relative_subpath, real_dir)?;
    ctx.visited.remove(real_dir);
    Ok(())
}

fn ingest_file(
    ctx: &mut WalkCtx<'_>,
    read_path: &Path,
    meta: Option<&fs::Metadata>,
    relative_subpath: String,
) -> Result<(), AddFilesFromDirError> {
    let stat;
    let meta = if let Some(meta) = meta {
        meta
    } else {
        stat = fs::metadata(read_path).map_err(|source| AddFilesFromDirError::Stat {
            path: read_path.to_path_buf(),
            source,
        })?;
        &stat
    };
    if !meta.is_file() {
        return Ok(());
    }
    let buffer = fs::read(read_path).map_err(|source| AddFilesFromDirError::ReadFile {
        path: read_path.to_path_buf(),
        source,
    })?;
    let mode = file_mode_from(meta);
    let (_path, hash) = ctx
        .store_dir
        .write_cas_file(&buffer, is_executable(mode))
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
