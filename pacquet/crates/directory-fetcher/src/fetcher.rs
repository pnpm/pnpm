//! Public entry point of [`crate`]: turn a source directory into a
//! relative-path → absolute-source-path map (`files_map`) plus the
//! manifest and `requires_build` flag downstream needs to decide
//! virtual-store layout and whether to run build scripts.
//!
//! Mirrors upstream's
//! [`createDirectoryFetcher`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L20-L37)
//! and [`fetchFromDir`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L50-L56).
//! Pacquet collapses the factory plus the per-call closure into a
//! single [`DirectoryFetcher`] struct because all the configuration
//! knobs (`include_only_package_files`, `resolve_symlinks`) are
//! per-fetch values in pacquet's install dispatch.

use crate::{error::DirectoryFetcherError, walker};
use pacquet_package_manifest::{pkg_requires_build, safe_read_package_json_from_dir};
use std::{collections::HashMap, path::PathBuf};

/// One directory-fetch request. The `directory` is the absolute
/// resolved path the caller wants packaged — upstream's
/// `path.resolve(opts.lockfileDir, resolution.directory)` happens at
/// the call site so this struct doesn't need to know about lockfile
/// layout. The two booleans match upstream's
/// [`CreateDirectoryFetcherOptions`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L15-L18):
///
/// - `include_only_package_files = true` → packlist mode
///   (`.npmignore` / `files` field / always-include filters).
/// - `resolve_symlinks = true` → follow symlinks via `realpath` (used
///   when `resolveSymlinksInInjectedDirs` is on).
pub struct DirectoryFetcher {
    pub directory: PathBuf,
    pub include_only_package_files: bool,
    pub resolve_symlinks: bool,
}

/// Result of [`DirectoryFetcher::run`]. Mirrors upstream's
/// [`FetchResult`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L41-L48)
/// minus `local: true` (implicit — pacquet routes locally-sourced
/// snapshots through this fetcher only) and `packageImportMethod`
/// (the install dispatcher encodes the import method by which slot it
/// writes to, not by a field on the fetcher output).
///
/// `manifest` is `None` when the directory has no `package.json` —
/// upstream supports this for the Bit-workspace shape documented at
/// [`directory-fetcher/src/index.ts:63-66`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L63-L66).
pub struct DirectoryFetchOutput {
    pub files_map: HashMap<String, PathBuf>,
    pub manifest: Option<serde_json::Value>,
    pub requires_build: bool,
}

impl DirectoryFetcher {
    pub fn run(&self) -> Result<DirectoryFetchOutput, DirectoryFetcherError> {
        let files_map = if self.include_only_package_files {
            walker::walk_package_files(&self.directory)?
        } else {
            walker::walk_all_files(&self.directory, self.resolve_symlinks)?
        };
        let manifest = safe_read_package_json_from_dir(&self.directory)
            .map_err(DirectoryFetcherError::ReadManifest)?;
        // Upstream's `pkgRequiresBuild(manifest, filesMap)` checks
        // (a) scripts.preinstall / install / postinstall on the
        // manifest, and (b) whether `binding.gyp` or `.hooks/*` is
        // in the *filtered* filesMap. Pacquet's
        // `pkg_requires_build(pkg_root)` does the same scripts check
        // on the manifest read from disk, but inspects
        // `binding.gyp` / `.hooks/` via the filesystem at `pkg_root`,
        // not the post-filter files_map. The two diverge only when
        // `include_only_package_files = true` AND the packlist
        // excludes `binding.gyp` / `.hooks/` from the published
        // tarball — uncommon, but a real parity gap. Documented;
        // revisit when a real package surfaces it.
        let requires_build = pkg_requires_build(&self.directory);
        Ok(DirectoryFetchOutput { files_map, manifest, requires_build })
    }
}
