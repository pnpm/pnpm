//! Walk a local package directory and produce the relative-path →
//! absolute-source-path map (`files_map`).
//!
//! Two modes:
//!
//! - [`walk_all_files`]: recursive walk, exclude `node_modules`, drop
//!   broken symlinks, optionally resolve symlinks via a real-path stat.
//! - [`walk_package_files`]: delegate to
//!   [`pnpm_git_fetcher::packlist`] for the npm-packlist filtered
//!   set.

use crate::error::DirectoryFetcherError;
use pnpm_package_manifest::safe_read_package_json_from_dir;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// Output of [`walk_all_files`] / [`walk_package_files`]: the
/// relative-path → absolute-source-path map a downstream CAS-write
/// pass reads from. Forward-slash separators on every host (matches
/// the rest of pacquet's CAS plumbing).
pub(crate) type FilesMap = HashMap<String, PathBuf>;

/// Recursive walk of `dir`, skipping `node_modules` at any depth and
/// dropping entries whose `stat` (or `realpath` under `resolve_symlinks`)
/// fails with `ENOENT`.
pub(crate) fn walk_all_files(
    dir: &Path,
    resolve_symlinks: bool,
    allow_path_escape: bool,
) -> Result<FilesMap, DirectoryFetcherError> {
    let mut out = FilesMap::new();
    let mut visited = HashSet::new();
    let confined_root = if allow_path_escape { None } else { Some(canonicalize_path(dir)?) };
    // Descending the resolved root rather than `dir` keeps the tree
    // being read the one the containment check approved: retargeting a
    // linked `dir` mid-walk would otherwise feed entries that are
    // ordinary files, and so never checked against the root at all.
    let root = confined_root.as_deref().unwrap_or(dir);
    walk_all_inner(root, "", resolve_symlinks, confined_root.as_deref(), &mut visited, &mut out)?;
    Ok(out)
}

fn is_linked_entry(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn walk_all_inner(
    dir: &Path,
    rel_prefix: &str,
    resolve_symlinks: bool,
    confined_root: Option<&Path>,
    visited: &mut HashSet<PathBuf>,
    out: &mut FilesMap,
) -> Result<(), DirectoryFetcherError> {
    // Symlink-cycle guard. Pnpm's directory-fetcher recurses without
    // a visited-set so a `foo -> .` (or any ancestor-pointing
    // symlink) sinks the whole walk into infinite recursion until the
    // path exceeds OS limits and `read_dir` finally errors with
    // ENAMETOOLONG. Stack overflow is also reachable on platforms
    // where the path-too-long error has a higher ceiling than the
    // default Rust stack. Skip-on-revisit instead, matching the
    // pattern `pnpm_git_fetcher::packlist` already uses for
    // `bundleDependencies` cycles. The check is keyed off
    // `fs::canonicalize` so an unresolved symlink and its target
    // share one entry; canonicalisation failure (permission denied,
    // for example) falls back to the raw path so the guard still
    // catches identity loops.
    let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical) {
        tracing::warn!(
            target: "pacquet::directory_fetcher",
            dir = %dir.display(),
            "symlink cycle: directory already visited at this canonical path; skipping",
        );
        return Ok(());
    }
    let entries = fs::read_dir(dir)
        .map_err(|source| DirectoryFetcherError::Io { dir: dir.display().to_string(), source })?;
    for entry in entries {
        let entry = entry.map_err(|source| DirectoryFetcherError::Io {
            dir: dir.display().to_string(),
            source,
        })?;
        let Some(rel) = walked_relative_path(&entry, rel_prefix) else {
            continue;
        };
        let Some(resolved) = resolve_entry(&entry.path(), resolve_symlinks, confined_root)? else {
            continue;
        };
        if resolved.metadata.is_dir() {
            walk_all_inner(&resolved.path, &rel, resolve_symlinks, confined_root, visited, out)?;
        } else {
            out.insert(rel, resolved.path);
        }
    }
    Ok(())
}

/// The forward-slash path an entry gets in the files map, or `None` for one
/// the walk passes over.
///
/// A non-UTF-8 name cannot round-trip through that map, and `node_modules` is
/// never part of a fetched directory.
fn walked_relative_path(entry: &fs::DirEntry, rel_prefix: &str) -> Option<String> {
    let file_name = entry.file_name();
    let file_name = file_name.to_str()?;
    if file_name == "node_modules" {
        return None;
    }
    if rel_prefix.is_empty() {
        return Some(file_name.to_string());
    }
    Some(format!("{rel_prefix}/{file_name}"))
}

struct ResolvedEntry {
    /// The path to use as the source for hardlinking / CAS-write.
    /// Under `resolve_symlinks`, this is the realpath; otherwise it's
    /// the lstat'd path the caller handed in.
    path: PathBuf,
    metadata: Metadata,
}

/// Stat a single entry.
fn resolve_entry(
    path: &Path,
    resolve_symlinks: bool,
    confined_root: Option<&Path>,
) -> Result<Option<ResolvedEntry>, DirectoryFetcherError> {
    if let Some(root) = confined_root {
        return resolve_confined_entry(path, root);
    }
    if resolve_symlinks {
        return resolve_followed_entry(path);
    }
    // Use `fs::metadata` (Rust's `stat`, not `lstat`): it follows symlinks
    // for the *type* decision but reports a broken symlink's ENOENT, which
    // the caller treats as "skip".
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(ResolvedEntry { path: path.to_path_buf(), metadata })),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(skip_broken_symlink(path)),
        Err(source) => Err(DirectoryFetcherError::Io { dir: path.display().to_string(), source }),
    }
}

/// Resolve an entry that must stay inside `root`. A link pointing out of the
/// directory is an error, not a skip.
fn resolve_confined_entry(
    path: &Path,
    root: &Path,
) -> Result<Option<ResolvedEntry>, DirectoryFetcherError> {
    let Some(lstat) = stat_or_skip(path, |path| fs::symlink_metadata(path))? else {
        return Ok(None);
    };
    if !is_linked_entry(&lstat) {
        return Ok(Some(ResolvedEntry { path: path.to_path_buf(), metadata: lstat }));
    }
    let Some(real) = stat_or_skip(path, |path| fs::canonicalize(path))? else {
        return Ok(None);
    };
    if !real.starts_with(root) {
        return Err(DirectoryFetcherError::PathOutsideDirectory {
            path: path.to_path_buf(),
            directory: root.to_path_buf(),
        });
    }
    let Some(metadata) = stat_or_skip(&real, |path| fs::metadata(path))? else {
        return Ok(None);
    };
    Ok(Some(ResolvedEntry { path: real, metadata }))
}

/// Resolve an entry through its link target, skipping a broken symlink.
fn resolve_followed_entry(path: &Path) -> Result<Option<ResolvedEntry>, DirectoryFetcherError> {
    let Some(lstat) = stat_or_skip(path, |path| fs::symlink_metadata(path))? else {
        return Ok(None);
    };
    if !is_linked_entry(&lstat) {
        return Ok(Some(ResolvedEntry { path: path.to_path_buf(), metadata: lstat }));
    }
    let real = match fs::canonicalize(path) {
        Ok(real) => real,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(skip_broken_symlink(path)),
        Err(source) => {
            return Err(DirectoryFetcherError::Io { dir: path.display().to_string(), source });
        }
    };
    match fs::metadata(&real) {
        Ok(metadata) => Ok(Some(ResolvedEntry { path: real, metadata })),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(skip_broken_symlink(path)),
        Err(source) => Err(DirectoryFetcherError::Io { dir: real.display().to_string(), source }),
    }
}

/// Run one stat-like call, reading a vanished path as "skip this entry".
fn stat_or_skip<Stat, Value>(
    path: &Path,
    stat: Stat,
) -> Result<Option<Value>, DirectoryFetcherError>
where
    Stat: FnOnce(&Path) -> io::Result<Value>,
{
    match stat(path) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(DirectoryFetcherError::Io { dir: path.display().to_string(), source }),
    }
}

/// Note a symlink whose target is gone, which the walk passes over.
fn skip_broken_symlink<Entry>(path: &Path) -> Option<Entry> {
    tracing::debug!(
        target: "pacquet::directory_fetcher",
        broken_symlink = %path.display(),
        "skipping broken symlink",
    );
    None
}

pub(crate) fn resolve_paths_in_directory(
    directory: &Path,
    files_map: &mut FilesMap,
) -> Result<(), DirectoryFetcherError> {
    let root = canonicalize_path(directory)?;
    for path in files_map.values_mut() {
        let original = path.clone();
        let resolved = canonicalize_path(&original)?;
        if !resolved.starts_with(&root) {
            return Err(DirectoryFetcherError::PathOutsideDirectory {
                path: original,
                directory: root,
            });
        }
        *path = resolved;
    }
    Ok(())
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, DirectoryFetcherError> {
    fs::canonicalize(path)
        .map_err(|source| DirectoryFetcherError::Io { dir: path.display().to_string(), source })
}

/// Read the manifest for packlist filtering, run
/// [`pnpm_git_fetcher::packlist`], and absolutise each entry against
/// `dir`.
pub(crate) fn walk_package_files(dir: &Path) -> Result<FilesMap, DirectoryFetcherError> {
    // packlist requires *some* manifest; pass the JSON just read from
    // disk. When the manifest is missing the
    // packlist filter still works against an empty object (no `files`
    // field, no `bundleDependencies`), which collapses to "include
    // every walked file except always-excluded cruft".
    let manifest = safe_read_package_json_from_dir(dir)
        .map_err(DirectoryFetcherError::ReadManifest)?
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    let files =
        pnpm_git_fetcher::packlist(dir, &manifest).map_err(DirectoryFetcherError::Packlist)?;
    let mut out = FilesMap::with_capacity(files.len());
    for rel in files {
        let abs = dir.join(&rel);
        out.insert(rel, abs);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
