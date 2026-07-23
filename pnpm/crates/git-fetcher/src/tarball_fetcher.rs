//! Fetcher for `TarballResolution { gitHosted: true }` snapshots.
//!
//! By the time control reaches this fetcher, `pacquet-tarball` has
//! already downloaded the tarball, verified its integrity, and
//! imported its file set into the CAS — the dispatcher hands us the
//! resulting `HashMap<String, PathBuf>` mapping relative paths to CAS
//! file paths. From there we materialize the files into a writable
//! temp dir, run `preparePackage` to (potentially) execute the dep's
//! build scripts, run a packlist over the prepared tree, and
//! re-import the resulting file set into the CAS.
//!
//! Implementation notes:
//!
//! - **No two-slot store-index row.** Pacquet's tarball download path
//!   doesn't write a `\traw` row at this key, so on the fast path we
//!   synthesize the prepared row directly from the input `cas_paths`
//!   (no `fs::read`, no re-hash). When fast-path triggers and
//!   `should_be_built` is false, the synthesized row lands at the
//!   final key. The skipped re-import is the perf win; the orphan raw
//!   row (if pacquet-tarball ever starts writing one) is a separate
//!   cleanup follow-up.
//! - **Warnings route through `tracing::warn!`.** When `ignore_scripts`
//!   suppresses a needed build, pacquet logs a warning through
//!   `tracing` since pacquet's reporter model doesn't have a global
//!   warn channel.

use crate::{
    cas_io::{ImportedFiles, import_into_cas, materialize_into, synthesize_files_index},
    error::GitFetcherError,
    fetcher::GitFetchOutput,
    prepare_package::{AllowBuildRef, PreparePackageOptions, PreparedPackage, prepare_package},
};
use pacquet_executor::ScriptsPrependNodePath;
use pacquet_fs_packlist::packlist;
use pacquet_package_manifest::safe_read_package_json_from_dir;
use pacquet_reporter::Reporter;
use pacquet_store_dir::{PackageFilesIndex, StoreDir, StoreIndexWriter};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// One-shot fetcher for a single git-hosted tarball resolution.
///
/// The dispatcher constructs this *after* `pacquet-tarball` has
/// downloaded and CAS-imported the tarball, handing us the
/// `cas_paths` map. The shape lines up with [`crate::GitFetcher`] so
/// both `LockfileResolution::Git` and `LockfileResolution::Tarball {
/// gitHosted: true }` produce a [`GitFetchOutput`] the install
/// dispatcher consumes uniformly.
pub struct GitHostedTarballFetcher<'a> {
    /// Raw tarball files already in the CAS. Keys are forward-slash
    /// relative paths, values are absolute CAS paths.
    pub cas_paths: HashMap<String, PathBuf>,
    /// `path` field from the resolution. Git-hosted tarball
    /// resolutions can include a sub-path to pack only one directory
    /// of the extracted tree (matches the git fetcher's `path`).
    /// `None` packs the tarball root.
    pub path: Option<&'a str>,
    /// Routed through to [`crate::prepare_package()`]'s `allow_build`.
    pub allow_build: AllowBuildRef<'a>,
    pub ignore_scripts: bool,
    pub unsafe_perm: bool,
    pub user_agent: Option<&'a str>,
    pub scripts_prepend_node_path: ScriptsPrependNodePath,
    pub script_shell: Option<&'a Path>,
    pub node_execpath: Option<&'a Path>,
    pub npm_execpath: Option<&'a Path>,
    pub store_dir: &'a StoreDir,
    /// Used in log lines.
    pub package_id: &'a str,
    pub requester: &'a str,
    /// Install-scoped store-index writer; see the matching field on
    /// [`crate::GitFetcher`] for the rationale.
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Cache key the row lands at; always the git-hosted shape for
    /// this fetcher. See [`crate::GitFetcher::files_index_file`].
    pub files_index_file: &'a str,
}

impl GitHostedTarballFetcher<'_> {
    /// Run the fetcher. Blocks under
    /// [`tokio::task::block_in_place`] so the synchronous
    /// `preparePackage` work doesn't tie up the async runtime.
    pub async fn run<Reporter: self::Reporter>(self) -> Result<GitFetchOutput, GitFetcherError> {
        tokio::task::block_in_place(|| self.run_sync::<Reporter>())
    }

    fn run_sync<Reporter: self::Reporter>(self) -> Result<GitFetchOutput, GitFetcherError> {
        let temp = tempfile::tempdir().map_err(GitFetcherError::Io)?;
        let temp_location = temp.path();

        // Step 1: Materialize the CAS-resident files into a writable
        // working tree, through a per-file `fs::copy` because the
        // tarball download has already settled the CAS write side.
        materialize_into(&self.cas_paths, temp_location)?;

        // Step 2: Run `preparePackage` on the materialized tree. This
        // honors `allow_build`, runs `<pm>-install` + `prepublish` /
        // `prepack` / `publish` lifecycle scripts when needed, and
        // returns `pkg_dir` (which respects `self.path`) plus the
        // `should_be_built` flag.
        let empty_env: HashMap<String, String> = HashMap::new();
        let prepare_opts = PreparePackageOptions {
            allow_build: Box::new(|dep_path| (self.allow_build)(dep_path)),
            dep_path: self.package_id,
            ignore_scripts: self.ignore_scripts,
            unsafe_perm: self.unsafe_perm,
            user_agent: self.user_agent,
            scripts_prepend_node_path: self.scripts_prepend_node_path,
            script_shell: self.script_shell,
            node_execpath: self.node_execpath,
            npm_execpath: self.npm_execpath,
            extra_bin_paths: &[],
            extra_env: &empty_env,
        };
        // Pacquet preserves the underlying error through the miette
        // source chain — the install dispatcher's log line already
        // includes `package_id`, so the chain renders as "prepare
        // failed for `<pkg>` → `ERR_PNPM_PREPARE_PACKAGE` → underlying
        // lifecycle error". A dedicated context variant is a follow-up
        // if the rendered chain proves unclear.
        let PreparedPackage { pkg_dir, should_be_built } =
            prepare_package::<Reporter>(&prepare_opts, temp_location, self.path)
                .map_err(GitFetcherError::Prepare)?;

        // Warn when scripts were ignored on a package that needs
        // building.
        if self.ignore_scripts && should_be_built {
            tracing::warn!(
                target: "pacquet::git_hosted_tarball_fetcher",
                package_id = %self.package_id,
                "the git-hosted tarball package has to be built but the build scripts were ignored",
            );
        }

        // Step 3: Compute the packlist over the prepared tree. The
        // raw tarball typically ships everything from the git
        // checkout (build artifacts, source maps, test fixtures);
        // applying the packlist filter on the way back into CAS
        // matches the file set the package would publish.
        let manifest = safe_read_package_json_from_dir(&pkg_dir)
            .unwrap_or(None)
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        let files = packlist(&pkg_dir, &manifest).map_err(GitFetcherError::Packlist)?;

        // Step 4: Fast path — when nothing got filtered out AND
        // prepare didn't mutate the tree (no build needed, or scripts
        // ignored), the materialized files are byte-identical to the
        // CAS source. Re-hashing every entry through `import_into_cas`
        // would land them at the same CAS paths via hash-dedup, so the
        // work is wasted.
        //
        // `path.is_none()` is required because a sub-path means
        // `cas_paths` covers the whole monorepo while `files` covers
        // only the sub-package — the count match is a coincidence
        // there, not equivalence.
        let fast_path_eligible = self.path.is_none() && files.len() == self.cas_paths.len();
        if fast_path_eligible && !should_be_built {
            // Synthesize the row from `cas_paths`: pacquet's tarball
            // download doesn't write a `\traw` row at the same key, so
            // there's nothing to copy — but the CAS files themselves
            // are already in place, which is what `cas_paths` points at.
            if let Some(writer) = self.store_index_writer {
                let files_index = synthesize_files_index(&self.cas_paths)?;
                writer.queue(
                    self.files_index_file.to_string(),
                    PackageFilesIndex {
                        manifest: None,
                        requires_build: Some(false),
                        algo: "sha512".to_string(),
                        files: files_index,
                        side_effects: None,
                    },
                );
            }
            return Ok(GitFetchOutput { cas_paths: self.cas_paths, built: false });
        }
        if fast_path_eligible && self.ignore_scripts {
            // `should_be_built && ignore_scripts`: prepare skipped the
            // scripts (warning already logged above), so the
            // materialized tree is still byte-identical to the source.
            // Return the raw filesMap *without* writing a final-key
            // row, so subsequent installs re-check the build gate. This
            // keeps `--ignore-scripts` installs idempotent.
            return Ok(GitFetchOutput { cas_paths: self.cas_paths, built: should_be_built });
        }

        // Step 5: Slow path — re-import the filtered file set back
        // into CAS and hand the resulting map to the install dispatcher.
        let ImportedFiles { cas_paths, files_index } =
            import_into_cas(self.store_dir, &pkg_dir, &files)?;

        // Step 6: Queue a `PackageFilesIndex` row so a future install's
        // warm prefetch skips the materialize+prepare+packlist+re-import
        // pass entirely. The final row lands at the git-hosted
        // store-index key; the dispatcher already builds that key and
        // passes it via `files_index_file`.
        if let Some(writer) = self.store_index_writer {
            writer.queue(
                self.files_index_file.to_string(),
                PackageFilesIndex {
                    manifest: None,
                    requires_build: Some(should_be_built),
                    algo: "sha512".to_string(),
                    files: files_index,
                    side_effects: None,
                },
            );
        }

        Ok(GitFetchOutput { cas_paths, built: should_be_built })
    }
}

#[cfg(test)]
mod tests;
