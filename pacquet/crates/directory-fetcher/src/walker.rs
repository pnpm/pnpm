//! Walk a local package directory and produce the relative-path →
//! absolute-source-path map upstream's
//! [`fetchAllFilesFromDir`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L58-L106)
//! and [`fetchPackageFilesFromDir`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L150-L165)
//! return as `filesMap`.
//!
//! Two modes:
//!
//! - [`walk_all_files`] mirrors `fetchAllFilesFromDir`: recursive
//!   walk, exclude `node_modules`, drop broken symlinks, optionally
//!   resolve symlinks via `realFileStat`.
//! - [`walk_package_files`] mirrors `fetchPackageFilesFromDir`:
//!   delegate to [`pacquet_git_fetcher::packlist`] for the npm-packlist
//!   filtered set.

use crate::error::DirectoryFetcherError;
use pacquet_package_manifest::safe_read_package_json_from_dir;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
};

/// Output of [`walk_all_files`] / [`walk_package_files`]: the
/// relative-path → absolute-source-path map a downstream CAS-write
/// pass reads from. Forward-slash separators on every host (matches
/// the rest of pacquet's CAS plumbing).
pub(crate) type FilesMap = HashMap<String, PathBuf>;

/// Recursive walk of `dir`, skipping `node_modules` at any depth and
/// dropping entries whose `stat` (or `realpath` under `resolve_symlinks`)
/// fails with `ENOENT`.
///
/// Mirrors upstream's
/// [`_fetchAllFilesFromDir`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L78-L106).
pub(crate) fn walk_all_files(
    dir: &Path,
    resolve_symlinks: bool,
) -> Result<FilesMap, DirectoryFetcherError> {
    let mut out = FilesMap::new();
    let mut visited = HashSet::new();
    walk_all_inner(dir, "", resolve_symlinks, &mut visited, &mut out)?;
    Ok(out)
}

fn walk_all_inner(
    dir: &Path,
    rel_prefix: &str,
    resolve_symlinks: bool,
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
    // pattern `pacquet_git_fetcher::packlist` already uses for
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
        let file_name = entry.file_name();
        // Non-UTF-8 names can't round-trip through pacquet's forward-slash
        // relative-path map; skip them, matching upstream's implicit JS
        // string semantics.
        let Some(file_name_str) = file_name.to_str() else { continue };
        if file_name_str == "node_modules" {
            continue;
        }
        let entry_path = entry.path();
        let Some(resolved) = resolve_entry(&entry_path, resolve_symlinks)? else { continue };
        let rel = if rel_prefix.is_empty() {
            file_name_str.to_string()
        } else {
            format!("{rel_prefix}/{file_name_str}")
        };
        if resolved.metadata.is_dir() {
            walk_all_inner(&resolved.path, &rel, resolve_symlinks, visited, out)?;
        } else {
            out.insert(rel, resolved.path);
        }
    }
    Ok(())
}

struct ResolvedEntry {
    /// The path to use as the source for hardlinking / CAS-write.
    /// Under `resolve_symlinks`, this is the realpath; otherwise it's
    /// the lstat'd path the caller handed in.
    path: PathBuf,
    metadata: Metadata,
}

/// Stat a single entry. Returns `Ok(None)` when the entry is a broken
/// symlink (`ENOENT` after `realpath`) or — under non-`resolve_symlinks`
/// mode — when `fs::metadata` fails with `ENOENT`. Mirrors upstream's
/// `fileStat` / `realFileStat` at
/// [`directory-fetcher/src/index.ts:113-148`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L113-L148).
fn resolve_entry(
    path: &Path,
    resolve_symlinks: bool,
) -> Result<Option<ResolvedEntry>, DirectoryFetcherError> {
    if resolve_symlinks {
        let lstat = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(DirectoryFetcherError::Io { dir: path.display().to_string(), source });
            }
        };
        if !lstat.file_type().is_symlink() {
            return Ok(Some(ResolvedEntry { path: path.to_path_buf(), metadata: lstat }));
        }
        let real = match fs::canonicalize(path) {
            Ok(p) => p,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(
                    target: "pacquet::directory_fetcher",
                    broken_symlink = %path.display(),
                    "skipping broken symlink",
                );
                return Ok(None);
            }
            Err(source) => {
                return Err(DirectoryFetcherError::Io { dir: path.display().to_string(), source });
            }
        };
        let real_meta = match fs::metadata(&real) {
            Ok(m) => m,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(
                    target: "pacquet::directory_fetcher",
                    broken_symlink = %path.display(),
                    "skipping broken symlink",
                );
                return Ok(None);
            }
            Err(source) => {
                return Err(DirectoryFetcherError::Io { dir: real.display().to_string(), source });
            }
        };
        Ok(Some(ResolvedEntry { path: real, metadata: real_meta }))
    } else {
        // Upstream's `fileStat` uses `fs.stat` (not `lstat`), which
        // follows symlinks for the *type* decision but reports the
        // broken-symlink ENOENT as "skip". Match that: `fs::metadata`
        // is Rust's `stat` analog.
        match fs::metadata(path) {
            Ok(m) => Ok(Some(ResolvedEntry { path: path.to_path_buf(), metadata: m })),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(
                    target: "pacquet::directory_fetcher",
                    broken_symlink = %path.display(),
                    "skipping broken symlink",
                );
                Ok(None)
            }
            Err(source) => {
                Err(DirectoryFetcherError::Io { dir: path.display().to_string(), source })
            }
        }
    }
}

/// Read the manifest for packlist filtering, run
/// [`pacquet_git_fetcher::packlist`], and absolutise each entry against
/// `dir`. Mirrors upstream's `fetchPackageFilesFromDir` —
/// [`directory-fetcher/src/index.ts:150-165`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L150-L165).
pub(crate) fn walk_package_files(dir: &Path) -> Result<FilesMap, DirectoryFetcherError> {
    // packlist requires *some* manifest; pnpm's `fetchPackageFilesFromDir`
    // passes the JSON it just read. When the manifest is missing the
    // packlist filter still works against an empty object (no `files`
    // field, no `bundleDependencies`), which collapses to "include
    // every walked file except always-excluded cruft".
    let manifest = safe_read_package_json_from_dir(dir)
        .map_err(DirectoryFetcherError::ReadManifest)?
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    let files =
        pacquet_git_fetcher::packlist(dir, &manifest).map_err(DirectoryFetcherError::Packlist)?;
    let mut out = FilesMap::with_capacity(files.len());
    for rel in files {
        let abs = dir.join(&rel);
        out.insert(rel, abs);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
