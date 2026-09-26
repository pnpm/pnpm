//! Fetcher for `TarballResolution { gitHosted: true }` snapshots.
//!
//! By the time control reaches this fetcher, `pnpm-tarball` has
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
//!   row (if pnpm-tarball ever starts writing one) is a separate
//!   cleanup follow-up.
//! - **Warnings route through `tracing::warn!`.** When `ignore_scripts`
//!   suppresses a needed build, pacquet logs a warning through
//!   `tracing` since pacquet's reporter model doesn't have a global
//!   warn channel.

use crate::{
    cas_io::{ImportedFiles, import_into_cas, materialize_into, synthesize_files_index},
    error::GitFetcherError,
    fetcher::{GitFetchOutput, NO_EXTRA_ENV, packlist_of, queue_files_index},
    prepare_package::{AllowBuildRef, PreparePackageOptions, PreparedPackage, prepare_package},
};
use pnpm_reporter::Reporter;
use std::{collections::HashMap, path::PathBuf};

/// One-shot fetcher for a single git-hosted tarball resolution.
///
/// The dispatcher constructs this *after* `pnpm-tarball` has
/// downloaded and CAS-imported the tarball, handing us the
/// `cas_paths` map. The shape lines up with [`crate::GitFetcher`] so
/// both `LockfileResolution::Git` and `LockfileResolution::Tarball {
/// gitHosted: true }` produce a [`GitFetchOutput`] the install
/// dispatcher consumes uniformly.
pub struct GitHostedTarballFetcher<'a> {
    pub scripts: crate::PrepareScriptOptions<'a>,
    pub store: crate::GitStoreContext<'a>,
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
    /// Used in log lines; see the matching field on
    /// [`crate::GitFetcher`] for its other role.
    pub package_id: &'a str,
    pub requester: &'a str,
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
        let prepared =
            prepare_package::<Reporter>(&self.prepare_options(), temp_location, self.path)
                .map_err(GitFetcherError::Prepare)?;

        // Warn when scripts were ignored on a package that needs
        // building.
        if prepared.ignored_build {
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
        let files = packlist_of(&prepared.pkg_dir)?;

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
        let fast_path_eligible =
            self.path.is_none() && files.len() == self.cas_paths.len();
        if fast_path_eligible && (!prepared.should_be_built || prepared.ignored_build) {
            if self.store.index_writer.is_some()
                && (!prepared.ignored_build || !self.scripts.ignore)
            {
                let key = prepared.store_index_key(
                    self.store.files_index_file,
                    self.package_id,
                    self.scripts.ignore,
                );
                queue_files_index(
                    self.store.index_writer,
                    &key,
                    synthesize_files_index(&self.cas_paths)?,
                    prepared.should_be_built,
                );
            }
            return Ok(GitFetchOutput {
                cas_paths: self.cas_paths,
                built: prepared.should_be_built,
            });
        }

        self.store_prepared(&prepared, &files)
    }

    fn store_prepared(
        &self,
        prepared: &PreparedPackage,
        files: &[String],
    ) -> Result<GitFetchOutput, GitFetcherError> {
        // Step 5: Slow path — re-import the filtered file set back
        // into CAS and hand the resulting map to the install dispatcher.
        let ImportedFiles { cas_paths, files_index } =
            import_into_cas(self.store.dir, &prepared.pkg_dir, files)?;

        // Step 6: Queue a `PackageFilesIndex` row so a future install's
        // warm prefetch skips the materialize+prepare+packlist+re-import
        // pass entirely. The final row lands at the git-hosted
        // store-index key; the dispatcher already builds that key and
        // passes it via `files_index_file`.
        let files_index_file = prepared.store_index_key(
            self.store.files_index_file,
            self.package_id,
            self.scripts.ignore,
        );
        queue_files_index(
            self.store.index_writer,
            &files_index_file,
            files_index,
            prepared.should_be_built,
        );

        Ok(GitFetchOutput { cas_paths, built: prepared.should_be_built })
    }
}

impl<'a> GitHostedTarballFetcher<'a> {
    fn prepare_options(&self) -> PreparePackageOptions<'a> {
        let allow_build = self.allow_build;
        PreparePackageOptions {
            scripts: self.scripts,
            allow_build: Box::new(move |dep_path| allow_build(dep_path)),
            pkg_resolution_id: self.package_id,

            extra_bin_paths: &[],
            extra_env: &NO_EXTRA_ENV,
        }
    }
}

#[cfg(test)]
mod tests;
