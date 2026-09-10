mod sources;

use super::{
    InstallPackageBySnapshot, InstallPackageBySnapshotError, InstalledPackage,
    runtime::binary_variant_for_host,
};
use crate::{
    CreateVirtualDirBySnapshot, build_modules::exec_scripts_prepend_node_path,
    retry_config::retry_opts_from_config,
};
use pnpm_config::NodeLinker;
use pnpm_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{
    DirectoryResolution, LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry,
};
use pnpm_reporter::{LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter};
use pnpm_tarball::{IngestTarballToStore, MemCache, TarballError};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The per-call inputs of [`InstallPackageBySnapshot::tarball_cas_paths`],
/// alongside the install-scoped ones it reads off `self`.
/// What a custom fetcher made of the lockfile's resolution: a
/// replacement resolution when it declined or delegated, the files when
/// it fetched, neither when no fetcher is configured.
pub(super) struct CustomFetched {
    resolution: Option<LockfileResolution>,
    cas_paths: Option<HashMap<String, PathBuf>>,
}
pub(super) struct SnapshotFetch<'f> {
    package_key: &'f PackageKey,
    package_id: &'f str,
    /// The effective resolution, which a custom fetcher's `delegate`
    /// may have replaced.
    resolution: &'f LockfileResolution,
    download: &'f IngestTarballToStore<'f>,
}
#[derive(Clone, Copy)]
pub(super) struct SlotLink<'s> {
    package_key: &'s PackageKey,
    snapshot: &'s SnapshotEntry,
    package_id: &'s str,
    source_is_mutable: bool,
}
pub(super) struct TarballFetch<'a, AllowBuild> {
    download: &'a IngestTarballToStore<'a>,
    /// The effective resolution, which a custom fetcher's `delegate`
    /// may have replaced.
    resolution: &'a LockfileResolution,
    package_key: &'a PackageKey,
    package_id: &'a str,
    allow_build: &'a AllowBuild,
    scripts_prepend_node_path: ExecScriptsPrependNodePath,
}

/// Reuse an in-flight or completed background download through the
/// shared mem cache when one is given; otherwise fetch standalone. The
/// owned `HashMap` is cloned out of the shared `Arc` so the rest of the
/// pass keeps its by-value contract.
///
/// The caller passes a mem cache only for registry resolutions: those
/// are the only ones the background prefetchers populate — the pnpr
/// `TarballPrefetcher` and the resolve-time `PrefetchingResolver` both
/// key by `name@version`, and a remote tarball resolves with no
/// `name_ver`, so they skip it. Its only mem-cache entry comes from the
/// resolver's download-to-resolve, and a hit on that entry returns the
/// extraction without touching the store index. Taking the standalone
/// path instead keeps this pass reconciling the row itself, so a later
/// re-resolve finds the warm store whatever the resolver did or didn't
/// write.
pub(super) async fn download_tarball<Reporter: self::Reporter>(
    download: IngestTarballToStore<'_>,
    tarball_mem_cache: Option<&MemCache>,
    revision_addressed: bool,
) -> Result<HashMap<String, PathBuf>, TarballError> {
    let Some(mem_cache) = tarball_mem_cache else {
        return if revision_addressed {
            download.run_revision_addressed_without_mem_cache::<Reporter>().await
        } else {
            download.run_without_mem_cache::<Reporter>().await
        };
    };
    // `clone()` is cheap (refs + `Arc`s) and lets us retry through
    // `run_without_mem_cache` below if the shared download failed.
    let result = if revision_addressed {
        download.clone().run_revision_addressed_with_mem_cache::<Reporter>(mem_cache).await
    } else {
        download.clone().run_with_mem_cache::<Reporter>(mem_cache).await
    };
    match result {
        Ok(cas_paths) => Ok((*cas_paths).clone()),
        Err(TarballError::SiblingFetchFailed { .. }) if !revision_addressed => {
            download.run_without_mem_cache::<Reporter>().await
        }
        Err(err) => Err(err),
    }
}
pub(super) fn fetch_directory_resolution(
    workspace_root: &Path,
    dir_resolution: &DirectoryResolution,
    include_only_package_files: bool,
) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
    let directory = lexical_normalize(&workspace_root.join(&dir_resolution.directory));
    let output = pnpm_directory_fetcher::DirectoryFetcher {
        directory,
        include_only_package_files,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .map_err(InstallPackageBySnapshotError::DirectoryFetch)?;
    Ok(output.files_map)
}
/// `pnpm:progress` `resolved` for a frozen-lockfile snapshot the
/// cold-batch path is about to fetch: one event per (resolved)
/// package, fired before the fetch attempt. In pacquet's
/// frozen-lockfile path the lockfile *is* the resolution, so each
/// snapshot is "already resolved" by the time we reach this site.
///
/// Pulled out of [`InstallPackageBySnapshot::run`] so the
/// event-construction code is unit-testable; the call site itself
/// only fires when a non-empty cold-batch lockfile install runs,
/// which the existing test suite doesn't cover.
pub(super) fn emit_progress_resolved<Reporter: self::Reporter>(package_id: &str, requester: &str) {
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Resolved {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

impl InstallPackageBySnapshot<'_> {
    /// Execute the subroutine. Returns the fetched package's CAS file
    /// index — the map relative-archive-path → absolute-store-path
    /// that downstream consumers use to either populate a
    /// virtual-store slot (isolated) or import into a hoisted
    /// `node_modules/<alias>/` directly (hoisted) — together with
    /// whether that map points at mutable local source, which only
    /// this function can tell because a custom fetcher's `delegate`
    /// can replace the lockfile's resolution.
    ///
    /// Under [`NodeLinker::Isolated`] the slot has already been
    /// materialized by the time this returns (via
    /// [`CreateVirtualDirBySnapshot`]); the returned map is still
    /// useful to the caller for assembling the
    /// [`crate::CasPathsByPkgId`] index when a workspace mixes
    /// linkers in the future. Under [`NodeLinker::Hoisted`] no slot
    /// is created — the returned map is the only output the caller
    /// gets, and it's threaded into [`crate::link_hoisted_modules()`].
    pub async fn run<Reporter: self::Reporter>(
        &self,
        package_key: &PackageKey,
        metadata: &PackageMetadata,
        snapshot: &SnapshotEntry,
    ) -> Result<InstalledPackage, InstallPackageBySnapshotError> {
        // TODO: skip when already exists in store?
        let package_id = package_key.pkg_id();
        emit_progress_resolved::<Reporter>(&package_id, self.ctx.requester);

        let download = self.ingest(metadata, &package_id);
        let custom =
            self.custom_fetch::<Reporter>(package_key, metadata, &package_id, &download).await?;
        let resolution = custom.resolution.as_ref().unwrap_or(&metadata.resolution);
        // Derived from the effective resolution, not the lockfile's: a
        // custom fetcher's `delegate` can resolve to a directory, and
        // then the file map points at mutable source even though the
        // lockfile entry says otherwise.
        let source_is_mutable = matches!(resolution, LockfileResolution::Directory(_));
        let cas_paths = match custom.cas_paths {
            Some(paths) => paths,
            None => {
                self.fetch_cas_paths::<Reporter>(SnapshotFetch {
                    package_key,
                    package_id: &package_id,
                    resolution,
                    download: &download,
                })
                .await?
            }
        };
        self.link_slot::<Reporter>(
            SlotLink { package_key, snapshot, package_id: &package_id, source_is_mutable },
            &cas_paths,
        )?;
        Ok(InstalledPackage { cas_paths, source_is_mutable })
    }

    fn ingest<'d>(
        &'d self,
        metadata: &'d PackageMetadata,
        package_id: &'d str,
    ) -> IngestTarballToStore<'d> {
        let config = self.ctx.config;
        IngestTarballToStore {
            http_client: self.http_client,
            store_dir: &config.store_dir,
            store_index: self.store_index.cloned(),
            store_index_writer: self.store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
            verified_files_cache: Arc::clone(self.verified_files_cache),
            package_integrity: metadata.resolution.checkable_integrity(),
            package_unpacked_size: None,
            package_file_count: None,
            package_url: "",
            package_id,
            requester: self.ctx.requester,
            prefetched_cas_paths: self.prefetched_cas_paths,
            retry_opts: retry_opts_from_config(config),
            auth_headers: &config.auth_headers,
            ignore_file_pattern: None,
            offline: config.offline,
            progress_reported: self.progress_reported.cloned(),
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: None,
            },
        }
    }

    async fn fetch_cas_paths<Reporter: self::Reporter>(
        &self,
        fetch: SnapshotFetch<'_>,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        // Named local so both git fetchers can borrow it across their
        // `.await` without depending on temporary-lifetime extension.
        let allow_build = self.allow_build();
        match fetch.resolution {
            LockfileResolution::Tarball(_) | LockfileResolution::Registry(_) => {
                self.fetch_snapshot_tarball::<Reporter>(&fetch, &allow_build).await
            }
            LockfileResolution::Directory(dir_resolution) => {
                // Injected workspace dep (`file:./local-pkg` with
                // `dependenciesMeta[*].injected = true`). The source
                // dir resolves as
                // `path.resolve(opts.lockfileDir, resolution.directory)`
                // and the fetcher returns `local: true` with a
                // `filesMap` that points directly at the source files
                // (no CAFS write). The `files_map` keys are the
                // forward-slash relative paths, the values are the
                // source paths, and downstream `link_file` /
                // `import_indexed_dir` hardlink-or-copy from those
                // source paths into the slot / hoisted directory just
                // like they would from a CAS-resident entry.
                fetch_directory_resolution(
                    self.ctx.workspace_root,
                    dir_resolution,
                    !config.deploy_all_files,
                )
            }
            // Runtime artifacts (Node.js / Bun / Deno) — `Binary`
            // and `Variations` carry a `BinaryResolution` describing
            // the archive to fetch. `Variations` is the multi-
            // platform wrapper: pick the variant whose `targets`
            // includes the host triple, then route through the same
            // `BinaryResolution` extractor.
            LockfileResolution::Binary(binary) => {
                self.fetch_binary::<Reporter>(binary, fetch.package_key).await
            }
            LockfileResolution::Variations(variations) => {
                self.fetch_binary::<Reporter>(
                    binary_variant_for_host(
                        variations,
                        fetch.package_key,
                        self.runtime_platform_selector,
                    )?,
                    fetch.package_key,
                )
                .await
            }
            LockfileResolution::Git(git_resolution) => {
                self.fetch_git::<Reporter>(&fetch, git_resolution, &allow_build).await
            }
            // A custom-typed resolution cannot be materialized without
            // a custom fetcher that claims it.
            LockfileResolution::Custom(custom) => {
                Err(InstallPackageBySnapshotError::UnsupportedResolutionType {
                    resolution_type: custom.resolution_type.to_string(),
                })
            }
        }
    }

    async fn fetch_snapshot_tarball<Reporter: self::Reporter>(
        &self,
        fetch: &SnapshotFetch<'_>,
        allow_build: &(impl Fn(&str) -> bool + Send + Sync),
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        self.tarball_cas_paths::<Reporter>(TarballFetch {
            download: fetch.download,
            resolution: fetch.resolution,
            package_key: fetch.package_key,
            package_id: fetch.package_id,
            allow_build,
            scripts_prepend_node_path: exec_scripts_prepend_node_path(self.ctx.config),
        })
        .await
    }

    /// `AllowBuildPolicy::check` returns `None` when the package is
    /// neither allow-listed nor deny-listed. The default is deny
    /// (`None → false`): build scripts have to be explicitly opted in to
    /// run.
    fn allow_build(&self) -> impl Fn(&str) -> bool + Send + Sync {
        let policy = self.ctx.allow_build_policy;
        move |dep_path: &str| policy.check(dep_path).unwrap_or(false)
    }

    /// Under hoisted, the virtual-store slot would be unused —
    /// [`crate::link_hoisted_modules()`] consumes the CAS paths
    /// directly to materialize project-tree `node_modules/`
    /// directories, so any slot written here would only waste disk.
    /// Hoisted skips both `linkAllModules` (slot symlinks) and
    /// `linkAllPkgs` (slot file imports), and runs `linkHoistedModules`
    /// over the CAS paths instead.
    fn link_slot<Reporter: self::Reporter>(
        &self,
        slot: SlotLink<'_>,
        cas_paths: &HashMap<String, PathBuf>,
    ) -> Result<(), InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        if self.defer_link
            || !matches!(self.ctx.node_linker, NodeLinker::Isolated | NodeLinker::Pnp)
        {
            return Ok(());
        }
        CreateVirtualDirBySnapshot {
            layout: self.ctx.layout,
            cas_paths,
            import_method: config.package_import_method,
            logged_methods: self.ctx.logged_methods,
            requester: self.ctx.requester,
            package_id: slot.package_id,
            package_key: slot.package_key,
            snapshot: slot.snapshot,
            source_is_mutable: slot.source_is_mutable,
            force_import: false,
            include_optional_dependencies: self.include_optional_dependencies,
            symlink: config.symlink,
            skipped: self.skipped,
            // The non-deferred slot link runs only on the fresh
            // single-package path (no previous install to diff
            // against), so there are never obsolete children here.
            removed_aliases: &[],
            needs_build_marker_source: None,
            // The fresh single-package path materializes one slot;
            // there is no per-install batch to amortize a cache
            // layout over.
            dir_clone_cache: None,
            #[cfg(test)]
            link_concurrency_probe: self.link_concurrency_probe,
        }
        .run::<Reporter>()
        .map_err(InstallPackageBySnapshotError::CreateVirtualDir)
    }
}
