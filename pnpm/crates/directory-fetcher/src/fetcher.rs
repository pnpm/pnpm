//! Public entry point of [`crate`]: turn a source directory into a
//! relative-path → absolute-source-path map (`files_map`) plus the
//! manifest and `requires_build` flag downstream needs to decide
//! virtual-store layout and whether to run build scripts.
//!
//! The factory and the per-call closure are collapsed into a single
//! [`DirectoryFetcher`] struct because all the configuration knobs
//! (`include_only_package_files`, `resolve_symlinks`, path containment)
//! are per-fetch values in pacquet's install dispatch.

use crate::{
    error::DirectoryFetcherError,
    walker::{self, Symlinks},
};
use pnpm_package_manifest::{pkg_requires_build, safe_read_package_json_from_dir};
use std::{collections::HashMap, path::PathBuf};

/// One directory-fetch request. The `directory` is the absolute
/// resolved path the caller wants packaged — the
/// `resolve(lockfile_dir, resolution.directory)` join happens at the
/// call site so this struct doesn't need to know about lockfile
/// layout.
///
/// - `include_only_package_files = true` → packlist mode
///   (`.npmignore` / `files` field / always-include filters).
/// - `resolve_symlinks = true` → follow symlinks via `realpath` (used
///   when `resolveSymlinksInInjectedDirs` is on).
/// - `preserve_symlinks = true` → list a symlink by its own path, a
///   directory link included, for a caller that recreates it. Ignored
///   under `resolve_symlinks`.
/// - `allow_path_escape = false` → reject files whose real path leaves
///   the real path of `directory`.
pub struct DirectoryFetcher {
    pub directory: PathBuf,
    pub include_only_package_files: bool,
    pub resolve_symlinks: bool,
    pub preserve_symlinks: bool,
    pub allow_path_escape: bool,
}

/// Result of [`DirectoryFetcher::run`]: the `files_map`, the manifest,
/// and the `requires_build` flag. There is no `local` flag (implicit —
/// pacquet routes locally-sourced snapshots through this fetcher only)
/// and no `package_import_method` (the install dispatcher encodes the
/// import method by which slot it writes to, not by a field on the
/// fetcher output).
///
/// `manifest` is `None` when the directory has no `package.json`,
/// which is valid for the Bit-workspace shape.
pub struct DirectoryFetchOutput {
    pub files_map: HashMap<String, PathBuf>,
    pub manifest: Option<serde_json::Value>,
    pub requires_build: bool,
    /// Whether `directory` was there to walk. `false` only for the
    /// not-yet-built injected-dependency case above, where `files_map` is
    /// empty because there was nothing to read, not because the directory
    /// is genuinely empty. A caller that would otherwise force-reimport a
    /// mutable source (a directory dependency's content can change without
    /// the lockfile changing, so it re-imports on every install) must not
    /// do so from this empty, nonexistent-directory result: that would
    /// overwrite an already-materialized copy with nothing. Reimporting
    /// an existing, genuinely empty directory is still correct, so this
    /// flag, not `files_map.is_empty()`, is what the caller must branch
    /// on.
    pub exists: bool,
}

impl DirectoryFetcher {
    pub fn run(&self) -> Result<DirectoryFetchOutput, DirectoryFetcherError> {
        // An injected dependency whose packed content is the output of its
        // own lifecycle scripts (a project with `publishConfig.directory`
        // built by `prepare`) has no source directory on a fresh install:
        // the scripts run after linking, and the built output is imported
        // afterwards. Tolerate the not-yet-built directory here instead of
        // failing the walk, so the install can proceed to run the script.
        let exists = self.directory
            .try_exists()
            .map_err(|source| DirectoryFetcherError::Io {
                dir: self.directory.display().to_string(),
                source,
            })?;
        if !exists {
            let manifest = safe_read_package_json_from_dir(&self.directory)
                .map_err(DirectoryFetcherError::ReadManifest)?;
            return Ok(DirectoryFetchOutput {
                files_map: HashMap::new(),
                manifest,
                requires_build: false,
                exists: false,
            });
        }

        let symlinks = self.symlinks();
        let files_map = if self.include_only_package_files {
            let mut files_map = walker::walk_package_files(&self.directory)?;
            if !self.allow_path_escape {
                walker::resolve_paths_in_directory(&self.directory, &mut files_map, symlinks)?;
            }
            files_map
        } else {
            walker::walk_all_files(&self.directory, symlinks, self.allow_path_escape)?
        };
        let manifest = safe_read_package_json_from_dir(&self.directory)
            .map_err(DirectoryFetcherError::ReadManifest)?;
        // `pkg_requires_build(pkg_root)` checks scripts.preinstall /
        // install / postinstall on the manifest read from disk, and
        // inspects `binding.gyp` / `.hooks/` via the filesystem at
        // `pkg_root` rather than the post-filter files_map. That
        // distinction only matters when `include_only_package_files =
        // true` AND the packlist excludes `binding.gyp` / `.hooks/`
        // from the published tarball — uncommon, but a real gap.
        // Revisit when a real package surfaces it.
        let requires_build = pkg_requires_build(&self.directory);
        Ok(DirectoryFetchOutput { files_map, manifest, requires_build, exists: true })
    }

    fn symlinks(&self) -> Symlinks {
        if self.resolve_symlinks {
            Symlinks::Resolve
        } else if self.preserve_symlinks {
            Symlinks::Preserve
        } else {
            Symlinks::Keep
        }
    }
}

#[cfg(test)]
mod tests;
