use crate::{
    CreateVirtualDirBySnapshot, CreateVirtualDirError, CustomFetcherSession,
    build_modules::exec_scripts_prepend_node_path, custom_fetcher::CustomFetchOutcome,
    retry_config::retry_opts_from_config,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_config::{Config, NodeLinker};
use pnpm_directory_fetcher::DirectoryFetcherError;
use pnpm_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pnpm_fs::lexical_normalize;
use pnpm_git_fetcher::{GitFetchOutput, GitFetcher, GitFetcherError, GitHostedTarballFetcher};
use pnpm_graph_hasher::{host_arch, host_libc, host_platform};
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, LockfileResolution,
    PackageKey, PackageMetadata, PlatformSelector, SnapshotEntry, TarballUrlOptions,
    integrity_addressed_registry_tarball_url, is_git_hosted_tarball_url,
    is_integrity_addressed_registry_tarball_url, npm_tarball_url, registry_server_type,
    select_platform_variant,
};
use pnpm_network::ThrottledClient;
use pnpm_reporter::{LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use pnpm_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter,
    git_hosted_store_index_key,
};
use pnpm_tarball::{
    IgnoreEntryFilter, IngestTarballToStore, IngestZipArchiveToStore, MemCache, PrefetchedCasPaths,
    SharedReportedProgressKeys, TarballError,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

/// The running pnpm, which a git-hosted dependency's build is given so it
/// can install with the package manager it asks for.
///
/// `None` when the executable is something other than pnpm itself — the
/// Node.js addon runs this code inside `node`, where there is no pnpm
/// binary to forward to — and the build then falls back to whatever
/// package managers the host has installed.
static PNPM_EXECPATH: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    let path = std::env::current_exe().ok()?;
    let stem = path.file_stem()?.to_str()?;
    (stem == "pnpm").then_some(path)
});

/// Downloads a package tarball, extracts it, installs it to a virtual
/// dir, then creates the symlink layout for the package. CAS file
/// import and symlink creation run concurrently via `rayon::join`
/// inside [`CreateVirtualDirBySnapshot::run`].
///
/// Holds only what every snapshot of one install shares — the clients,
/// the store handles, the layout, the policies — so it is built once
/// and each snapshot is passed to [`Self::run`].
#[derive(Clone, Copy)]
pub struct InstallPackageBySnapshot<'a> {
    pub ctx: &'a crate::InstallContext<'a>,
    pub http_client: &'a ThrottledClient,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Install-scoped batched cache lookup result. See
    /// [`pnpm_tarball::prefetch_cas_paths`].
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Install-scoped shared in-flight tarball cache. When present, the
    /// registry/tarball download routes through
    /// [`IngestTarballToStore::run_with_mem_cache`] so it parks on (or
    /// reuses) a download already in flight or completed for the same
    /// URL, rather than racing a second fetch of the same bytes. Both
    /// background prefetchers feed it: the pnpr client's
    /// `TarballPrefetcher` (frozen materialization) and the
    /// fresh-resolve path's `PrefetchingResolver` (cold
    /// batch). `None` keeps the standalone `run_without_mem_cache`
    /// path for installs with no prefetcher (e.g. a plain
    /// `--frozen-lockfile` without pnpr).
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
    /// Install-scoped package-status progress dedupe. Shared with the
    /// resolve-time prefetcher on the fresh path so the cold fallback
    /// does not double-count a package whose early prefetch already
    /// emitted `fetched` or `found_in_store`.
    pub progress_reported: Option<&'a SharedReportedProgressKeys>,
    /// Install-scoped `verifiedFilesCache` shared across every
    /// per-snapshot fetch. See `IngestTarballToStore::verified_files_cache`
    /// for the rationale.
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Snapshots the installability pass ruled out on this host.
    pub skipped: &'a crate::SkippedSnapshots,
    pub include_optional_dependencies: bool,
    /// Platform triple used to select a runtime archive. This is the host
    /// triple unless `supportedArchitectures` targets another platform.
    pub runtime_platform_selector: &'a PlatformSelector,
    /// Custom fetchers from the pnpmfile's `fetchers` export.
    /// Consulted before the built-in resolution-type dispatch; `None`
    /// when no pnpmfile exports fetchers.
    pub custom_fetcher_session: Option<&'a Arc<CustomFetcherSession>>,
    /// When `true`, return the fetched CAS paths without populating the
    /// virtual-store slot ([`CreateVirtualDirBySnapshot`]) — the caller
    /// links them itself in a separate parallel pass. The cold batch in
    /// [`crate::CreateVirtualStore`] sets this so the per-snapshot
    /// download futures don't each run a *blocking* `rayon::join` link
    /// inside the cooperative `try_join_all` task, which would serialize
    /// the links one-at-a-time; instead every slot links concurrently
    /// once its tarball is in the store. No effect under
    /// [`NodeLinker::Hoisted`], which never writes virtual-store slots.
    pub defer_link: bool,
    #[cfg(test)]
    pub link_concurrency_probe:
        Option<&'a crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe>,
}

/// Error type of [`InstallPackageBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallPackageBySnapshotError {
    #[diagnostic(transparent)]
    DownloadTarball(#[error(source)] TarballError),

    #[diagnostic(transparent)]
    CreateVirtualDir(#[error(source)] CreateVirtualDirError),

    /// A plain remote tarball the lockfile pins no `integrity` for.
    /// Message and code mirror the TypeScript
    /// `assertFetchableResolution` in
    /// `pnpm11/installing/package-requester/src/packageRequester.ts`.
    /// See [`unverified_fetch_is_allowed`] for the shapes that are
    /// exempt.
    #[display(
        "Cannot fetch package \"{package_key}\" from the lockfile: it has no \"integrity\" field, so the downloaded tarball cannot be verified. Run a fresh install to repair the lockfile."
    )]
    #[diagnostic(
        code(ERR_PNPM_MISSING_TARBALL_INTEGRITY),
        help(
            "Re-resolving the entry is what records the missing hash: run `pnpm clean --lockfile` and then `pnpm install`."
        )
    )]
    MissingTarballIntegrity { package_key: String },

    #[display(
        "Cannot install package \"{package_key}\": its registry prefix '{registry_name}:' is not declared by the registries setting."
    )]
    #[diagnostic(
        code(ERR_PNPM_MISSING_NAMED_REGISTRY),
        help("Add a registries entry with \"prefix: {registry_name}\" to pnpm-workspace.yaml.")
    )]
    MissingNamedRegistry { package_key: String, registry_name: String },

    #[display(
        "Cannot install package \"{package_key}\": its lockfile entry with a revision {reason}."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_REVISION))]
    InvalidTarballRevision { package_key: String, reason: &'static str },

    #[display(
        "Package `{package_key}` uses a `{resolution_kind}` resolution, which pnpm does not yet support."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UNSUPPORTED_RESOLUTION))]
    UnsupportedResolution { package_key: String, resolution_kind: &'static str },

    /// Failure from either git fetcher: the git-CLI path for
    /// `type: git` resolutions (clone / checkout / preparePackage /
    /// CAS import) or the git-hosted-tarball post-pass for
    /// `TarballResolution { gitHosted: true }` (materialize /
    /// preparePackage / packlist / re-import). Both share the same
    /// `GitFetcherError` taxonomy because they share `prepare_package`,
    /// `packlist`, and the CAS-import helpers; the variant covers
    /// every fetcher path that exits through `pnpm-git-fetcher`.
    #[diagnostic(transparent)]
    GitFetch(#[error(source)] GitFetcherError),

    /// Failure from the directory fetcher: walking the source
    /// directory of an injected workspace dep, reading its manifest,
    /// or running the npm-packlist filter for
    /// `includeOnlyPackageFiles` mode.
    #[diagnostic(transparent)]
    DirectoryFetch(#[error(source)] DirectoryFetcherError),

    /// A custom fetcher from the pnpmfile threw or returned an error.
    #[display("Custom fetcher failed: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CUSTOM_FETCHER_FAILED))]
    CustomFetcher(#[error(not(source))] String),

    #[display(
        "Custom fetcher delegated package \"{package_id}\" to a resolution that cannot verify its locked integrity"
    )]
    #[diagnostic(code(ERR_PNPM_TARBALL_INTEGRITY))]
    CustomFetcherIntegrityMismatch { package_id: String },

    /// A custom-typed resolution reached the built-in dispatch — no
    /// pnpmfile custom fetcher claimed it. Message and code mirror the
    /// TypeScript `pickFetcher` in
    /// `pnpm11/fetching/pick-fetcher/src/index.ts`.
    #[display(
        "Cannot fetch dependency with custom resolution type \"{resolution_type}\". Custom resolutions must be handled by custom fetchers."
    )]
    #[diagnostic(code(ERR_PNPM_UNSUPPORTED_RESOLUTION_TYPE))]
    UnsupportedResolutionType { resolution_type: String },

    /// No variant in a [`LockfileResolution::Variations`] matches the
    /// selected triple `(os, cpu, libc?)`. Surfaces with that triple
    /// plus the list of advertised target triples so the user can see
    /// at a glance whether they're running on an unsupported platform
    /// or whether the lockfile was generated without the host's
    /// architecture in mind.
    #[display(
        "Package `{package_key}` is a runtime dependency, but none of its declared variants matches the selected triple ({selected_target}). Available variants: {available_targets}"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_MATCHING_PLATFORM_VARIANT))]
    NoMatchingPlatformVariant {
        package_key: String,
        selected_target: String,
        /// Pre-rendered list of the lockfile's advertised target
        /// triples, formatted as `os/cpu[+libc]`. Lives in the error
        /// payload rather than the lockfile (which is borrowed from
        /// the install request) so the error stays cheap to construct
        /// at the rejection site and isn't tied to the lockfile's
        /// lifetime.
        available_targets: String,
    },

    /// A variant inside a [`LockfileResolution::Variations`] carries
    /// a resolution other than [`LockfileResolution::Binary`].
    /// The lockfile contract guarantees variants are atomic
    /// `BinaryResolution`s; this variant catches lockfile corruption
    /// or a future shape pacquet doesn't recognise rather than
    /// silently routing through and confusing the install pipeline.
    #[display(
        "Package `{package_key}` carries a runtime variant whose inner resolution is `{inner_kind}` rather than `binary`; pnpm only knows how to install binary-shaped variants."
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_VARIANT_HAS_NON_BINARY_RESOLUTION))]
    VariantHasNonBinaryResolution { package_key: String, inner_kind: &'static str },

    /// Serializing the synthesized runtime `package.json` failed.
    /// The manifest is a small fixed-shape JSON object (`name`,
    /// `version`, `bin`); `serde_json` rejects this only on a
    /// numeric or struct value the writer can't render, which can't
    /// happen for the three string-typed fields we pass it.
    /// Surfaces as a typed error rather than a panic so a future
    /// shape change to [`BinarySpec`] doesn't crash an install.
    #[display(
        "Failed to serialize the synthesized package.json for runtime entry `{package_key}`: {error}"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_SYNTHESIZE_RUNTIME_MANIFEST))]
    SynthesizeRuntimeManifest {
        package_key: String,
        #[error(source)]
        error: serde_json::Error,
    },
}

/// What installing one package produced.
#[derive(Debug)]
pub struct InstalledPackage {
    pub cas_paths: HashMap<String, PathBuf>,
    /// Whether [`Self::cas_paths`] points at mutable local source
    /// rather than immutable content-addressed entries. See
    /// [`crate::CreateVirtualDirBySnapshot::source_is_mutable`].
    pub source_is_mutable: bool,
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

    async fn custom_fetch<Reporter: self::Reporter>(
        &self,
        package_key: &PackageKey,
        metadata: &PackageMetadata,
        package_id: &str,
        download: &IngestTarballToStore<'_>,
    ) -> Result<CustomFetched, InstallPackageBySnapshotError> {
        let Some(session) = self.custom_fetcher_session else {
            return Ok(CustomFetched { resolution: None, cas_paths: None });
        };
        let config = self.ctx.config;
        let opts = serde_json::json!({
            "pkg": {
                "name": package_key.name.to_string(),
                "version": metadata.version.clone()
                    .unwrap_or_else(|| package_key.suffix.version().to_string()),
            },
            "lockfileDir": self.ctx.workspace_root,
            "readManifest": true,
            "filesIndexFile": pnpm_store_dir::pick_store_index_key(
                metadata.resolution.checkable_integrity().map(ToString::to_string).as_deref(),
                false, package_id, !config.ignore_scripts,
            ),
        });
        Ok(match session.fetch::<Reporter>(download.clone(), &metadata.resolution, opts).await? {
            CustomFetchOutcome::Declined(resolution)
            | CustomFetchOutcome::Delegate { delegate: resolution, .. } => {
                CustomFetched { resolution: Some(resolution), cas_paths: None }
            }
            CustomFetchOutcome::Fetched { tarball, .. } => {
                CustomFetched { resolution: None, cas_paths: Some(tarball.files_map.clone()) }
            }
        })
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
                self.tarball_cas_paths::<Reporter>(TarballFetch {
                    download: fetch.download,
                    resolution: fetch.resolution,
                    package_key: fetch.package_key,
                    package_id: fetch.package_id,
                    allow_build: &allow_build,
                    scripts_prepend_node_path: exec_scripts_prepend_node_path(config),
                })
                .await
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

    /// `AllowBuildPolicy::check` returns `None` when the package is
    /// neither allow-listed nor deny-listed. The default is deny
    /// (`None → false`): build scripts have to be explicitly opted in to
    /// run.
    fn allow_build(&self) -> impl Fn(&str) -> bool + Send + Sync {
        let policy = self.ctx.allow_build_policy;
        move |dep_path: &str| policy.check(dep_path).unwrap_or(false)
    }

    async fn fetch_binary<Reporter: self::Reporter>(
        &self,
        binary: &BinaryResolution,
        package_key: &PackageKey,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        fetch_binary_resolution_to_cas::<Reporter>(
            binary,
            self.http_client,
            self.ctx.config,
            self.store_index,
            self.store_index_writer,
            self.verified_files_cache,
            self.prefetched_cas_paths,
            package_key,
            self.ctx.requester,
            archive_filter_for(package_key),
        )
        .await
    }

    async fn fetch_git<Reporter: self::Reporter>(
        &self,
        fetch: &SnapshotFetch<'_>,
        git_resolution: &pnpm_lockfile::GitResolution,
        allow_build: &(impl Fn(&str) -> bool + Send + Sync),
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        // Same `built = !ignore_scripts` rationale as the git-hosted
        // tarball branch — key shape stays in lock-step with
        // `snapshot_cache_key`.
        let built = !config.ignore_scripts;
        let files_index_file = git_hosted_store_index_key(fetch.package_id, built);
        let package_name = fetch.package_key.name.to_string();
        let GitFetchOutput { cas_paths, built: _built } = GitFetcher {
            repo: &git_resolution.repo,
            commit: &git_resolution.commit,
            path: git_resolution.path.as_deref(),
            git_shallow_hosts: &config.git_shallow_hosts,
            allow_build,
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            user_agent: Some(&config.user_agent),
            scripts_prepend_node_path: exec_scripts_prepend_node_path(config),
            script_shell: None,
            node_execpath: None,
            npm_execpath: None,
            pnpm_execpath: PNPM_EXECPATH.as_deref(),
            store_dir: &config.store_dir,
            package_id: fetch.package_id,
            package_name: &package_name,
            requester: self.ctx.requester,
            store_index_writer: self.store_index_writer,
            files_index_file: &files_index_file,
            git_bin: None,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::GitFetch)?;
        Ok(cas_paths)
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

/// The per-call inputs of [`InstallPackageBySnapshot::tarball_cas_paths`],
/// alongside the install-scoped ones it reads off `self`.
/// What a custom fetcher made of the lockfile's resolution: a
/// replacement resolution when it declined or delegated, the files when
/// it fetched, neither when no fetcher is configured.
struct CustomFetched {
    resolution: Option<LockfileResolution>,
    cas_paths: Option<HashMap<String, PathBuf>>,
}

struct SnapshotFetch<'f> {
    package_key: &'f PackageKey,
    package_id: &'f str,
    /// The effective resolution, which a custom fetcher's `delegate`
    /// may have replaced.
    resolution: &'f LockfileResolution,
    download: &'f IngestTarballToStore<'f>,
}

#[derive(Clone, Copy)]
struct SlotLink<'s> {
    package_key: &'s PackageKey,
    snapshot: &'s SnapshotEntry,
    package_id: &'s str,
    source_is_mutable: bool,
}

struct TarballFetch<'a, AllowBuild> {
    download: &'a IngestTarballToStore<'a>,
    /// The effective resolution, which a custom fetcher's `delegate`
    /// may have replaced.
    resolution: &'a LockfileResolution,
    package_key: &'a PackageKey,
    package_id: &'a str,
    allow_build: &'a AllowBuild,
    scripts_prepend_node_path: ExecScriptsPrependNodePath,
}

impl InstallPackageBySnapshot<'_> {
    /// Fetch a tarball- or registry-shaped resolution into the store and
    /// return its CAS paths.
    async fn tarball_cas_paths<Reporter: self::Reporter>(
        &self,
        fetch: TarballFetch<'_, impl Fn(&str) -> bool + Send + Sync>,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        let revision_addressed = match fetch.resolution {
            LockfileResolution::Tarball(tarball) => tarball.revision.is_some(),
            LockfileResolution::Registry(registry) => registry.revision.is_some(),
            _ => false,
        };
        let (tarball_url, integrity) =
            tarball_url_and_integrity(fetch.resolution, fetch.package_key, config)?;
        let tarball_url = local_file_tarball_install_url(tarball_url, self.ctx.workspace_root);
        let download = IngestTarballToStore {
            package_url: &tarball_url,
            package_integrity: integrity,
            ..fetch.download.clone()
        };
        let raw_cas_paths = download_tarball::<Reporter>(
            download,
            self.tarball_mem_cache
                .filter(|_| matches!(fetch.resolution, LockfileResolution::Registry(_)))
                .map(std::convert::AsRef::as_ref),
            revision_addressed,
        )
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?;
        match fetch.resolution {
            LockfileResolution::Tarball(tarball) if tarball.is_git_hosted() => {
                self.prepare_git_hosted::<Reporter>(&fetch, tarball, raw_cas_paths).await
            }
            _ => Ok(raw_cas_paths),
        }
    }

    /// The git-hosted prepare+packlist pass: a `gitHosted: true` tarball
    /// routes through `gitHostedTarballFetcher` rather than the plain
    /// `remoteTarballFetcher`, because the host's archive endpoint
    /// doesn't run `prepare`/`prepublish*` and the file set typically
    /// needs packlist filtering.
    async fn prepare_git_hosted<Reporter: self::Reporter>(
        &self,
        fetch: &TarballFetch<'_, impl Fn(&str) -> bool + Send + Sync>,
        tarball: &pnpm_lockfile::TarballResolution,
        cas_paths: HashMap<String, PathBuf>,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let config = self.ctx.config;
        // `built` tracks `!ignore_scripts`, in lock-step with the key
        // shape `snapshot_cache_key` produces — otherwise the prefetch
        // and the write would address different slots. Under
        // `--ignore-scripts` the git-hosted `prepare` is suppressed too,
        // matching pnpm's `ignoreScripts`.
        let files_index_file = git_hosted_store_index_key(fetch.package_id, !config.ignore_scripts);
        let GitFetchOutput { cas_paths, built: _built } = GitHostedTarballFetcher {
            cas_paths,
            path: tarball.path.as_deref(),
            allow_build: fetch.allow_build,
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            user_agent: Some(&config.user_agent),
            scripts_prepend_node_path: fetch.scripts_prepend_node_path,
            script_shell: None,
            node_execpath: None,
            npm_execpath: None,
            pnpm_execpath: PNPM_EXECPATH.as_deref(),
            store_dir: &config.store_dir,
            package_id: fetch.package_id,
            requester: self.ctx.requester,
            store_index_writer: self.store_index_writer,
            files_index_file: &files_index_file,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::GitFetch)?;
        Ok(cas_paths)
    }
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
async fn download_tarball<Reporter: self::Reporter>(
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

/// The archive a `Variations` resolution offers for this host.
///
/// A platform asset resolution is always atomic (`BinaryResolution`);
/// pacquet's type widens to the full `LockfileResolution` for serde
/// uniformity but [`select_platform_variant`]'s docs spell out that
/// nested `Variations` would just route their picked variant's inner
/// shape back through the resolution dispatcher (no infinite recursion,
/// because this function does not call back into the variant selector).
/// Only `Binary` is recognised; anything else is either a corrupt
/// lockfile or a future shape pacquet hasn't learned about yet, so it is
/// rejected loudly rather than silently routed through.
fn binary_variant_for_host<'a>(
    variations: &'a pnpm_lockfile::VariationsResolution,
    package_key: &PackageKey,
    runtime_platform_selector: &pnpm_lockfile::PlatformSelector,
) -> Result<&'a pnpm_lockfile::BinaryResolution, InstallPackageBySnapshotError> {
    let Some(variant) = select_platform_variant(&variations.variants, runtime_platform_selector)
    else {
        return Err(InstallPackageBySnapshotError::NoMatchingPlatformVariant {
            package_key: package_key.to_string(),
            selected_target: format!(
                "os = `{}`, cpu = `{}`, libc = `{:?}`",
                runtime_platform_selector.os,
                runtime_platform_selector.cpu,
                runtime_platform_selector.libc,
            ),
            available_targets: render_variant_targets(&variations.variants),
        });
    };
    let LockfileResolution::Binary(binary) = &variant.resolution else {
        return Err(InstallPackageBySnapshotError::VariantHasNonBinaryResolution {
            package_key: package_key.to_string(),
            inner_kind: match &variant.resolution {
                LockfileResolution::Tarball(_) => "tarball",
                LockfileResolution::Registry(_) => "registry",
                LockfileResolution::Directory(_) => "directory",
                LockfileResolution::Git(_) => "git",
                LockfileResolution::Variations(_) => "variations",
                LockfileResolution::Custom(_) => "custom",
                // Already matched above; reach is unreachable.
                LockfileResolution::Binary(_) => "binary",
            },
        });
    };
    Ok(binary)
}

fn fetch_directory_resolution(
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

pub(crate) fn local_file_tarball_install_url<'a>(
    tarball_url: Cow<'a, str>,
    workspace_root: &Path,
) -> Cow<'a, str> {
    let Some(path) = tarball_url.strip_prefix("file:") else {
        return tarball_url;
    };
    if path.starts_with("//") || Path::new(path).is_absolute() {
        return tarball_url;
    }
    Cow::Owned(format!("file:{}", lexical_normalize(&workspace_root.join(path)).display()))
}

/// Resolve the tarball URL + integrity for tarball- and registry-shaped
/// resolutions. Factored out so the per-resolution-type dispatch in
/// [`InstallPackageBySnapshot::run`] reads top-down: each variant builds
/// its own `cas_paths`. Public because the pnpr server derives the same
/// URLs when it announces a verified frozen lockfile's tarballs to the
/// client — both sides must derive byte-identical URLs so the client's
/// prefetch mem-cache keys line up.
///
/// The integrity is `None` only for the shapes
/// [`unverified_fetch_is_allowed`] exempts — every other resolution whose
/// recorded integrity pins nothing (absent, or the empty SRI string an
/// edited lockfile can carry) is refused here rather than fetched
/// unchecked. See
/// [`pnpm_tarball::IngestTarballToStore::package_integrity`] for
/// what an unverified fetch does.
///
/// # Panics
///
/// On directory / git / binary / variations resolutions — callers gate
/// on the tarball/registry shapes first.
pub fn tarball_url_and_integrity<'a>(
    resolution: &'a LockfileResolution,
    package_key: &PackageKey,
    config: &'a Config,
) -> Result<(Cow<'a, str>, Option<&'a ssri::Integrity>), InstallPackageBySnapshotError> {
    match resolution {
        LockfileResolution::Tarball(tarball_resolution) => {
            tarball_resolution_url(resolution, tarball_resolution, package_key, config)
        }
        LockfileResolution::Registry(registry_resolution) => {
            registry_resolution_url(resolution, registry_resolution, package_key, config)
        }
        // Caller (`run`) only invokes this helper for the tarball /
        // registry arms; git, directory, binary, variations, and
        // custom resolutions never reach here.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => {
            unreachable!("tarball_url_and_integrity called with non-tarball resolution");
        }
    }
}

fn tarball_resolution_url<'a>(
    resolution: &'a LockfileResolution,
    tarball_resolution: &'a pnpm_lockfile::TarballResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<(Cow<'a, str>, Option<&'a ssri::Integrity>), InstallPackageBySnapshotError> {
    let tarball_url = tarball_resolution.tarball.as_str();
    let integrity = resolution.checkable_integrity();
    if tarball_resolution.revision.is_some() {
        check_tarball_revision(tarball_url, integrity, tarball_resolution, package_key, config)?;
    }
    if integrity.is_none() && !unverified_fetch_is_allowed(tarball_url) {
        return Err(InstallPackageBySnapshotError::MissingTarballIntegrity {
            package_key: package_key.to_string(),
        });
    }
    Ok((tarball_url.pipe(Cow::Borrowed), integrity))
}

/// A `revision`-carrying tarball resolution addresses a registry
/// tarball by its integrity, so the recorded URL must be exactly the
/// one that integrity derives against the package's registry.
fn check_tarball_revision(
    tarball_url: &str,
    integrity: Option<&ssri::Integrity>,
    tarball_resolution: &pnpm_lockfile::TarballResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<(), InstallPackageBySnapshotError> {
    if tarball_url.starts_with("file:") || tarball_resolution.is_git_hosted() {
        return Err(invalid_tarball_revision(package_key, "does not identify a registry tarball"));
    }
    let Some(integrity) = integrity else {
        return Err(invalid_tarball_revision(package_key, "has invalid or missing integrity"));
    };
    let (registry, _) = registry_and_version(package_key, config)?;
    if !is_integrity_addressed_registry_tarball_url(tarball_url, integrity, &registry) {
        return Err(invalid_tarball_revision(package_key, "has a mismatched tarball URL"));
    }
    Ok(())
}

fn registry_resolution_url<'a>(
    resolution: &'a LockfileResolution,
    registry_resolution: &pnpm_lockfile::RegistryResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<(Cow<'a, str>, Option<&'a ssri::Integrity>), InstallPackageBySnapshotError> {
    let Some(integrity) = resolution.checkable_integrity() else {
        if registry_resolution.revision.is_some() {
            return Err(invalid_tarball_revision(package_key, "has invalid or missing integrity"));
        }
        return Err(InstallPackageBySnapshotError::MissingTarballIntegrity {
            package_key: package_key.to_string(),
        });
    };
    let (registry, version) = registry_and_version(package_key, config)?;
    let tarball_url = match registry_resolution.revision {
        Some(_) => {
            integrity_addressed_registry_tarball_url(integrity, &registry).ok_or_else(|| {
                invalid_tarball_revision(package_key, "has invalid or missing integrity")
            })?
        }
        None => npm_tarball_url(
            &package_key.name.to_string(),
            &version,
            TarballUrlOptions {
                registry: &registry,
                server_type: registry_server_type(&config.registry_options_by_url, &registry),
            },
        ),
    };
    Ok((Cow::Owned(tarball_url), Some(integrity)))
}

fn registry_and_version(
    package_key: &PackageKey,
    config: &Config,
) -> Result<(String, String), InstallPackageBySnapshotError> {
    if let Some((registry_name, version)) = package_key.suffix.registry_qualified() {
        let registry = pnpm_resolving_npm_resolver::BUILTIN_REGISTRIES_BY_PREFIX
            .iter()
            .find(|(name, _)| *name == registry_name)
            .map(|(_, url)| (*url).to_string())
            .pipe(|builtin| config.registries_by_prefix.get(registry_name).cloned().or(builtin))
            .ok_or_else(|| InstallPackageBySnapshotError::MissingNamedRegistry {
                package_key: package_key.to_string(),
                registry_name: registry_name.to_string(),
            })?;
        return Ok((registry, version.to_string()));
    }
    let name = package_key.name.to_string();
    let registries: HashMap<String, String> = config.resolved_registries().into_iter().collect();
    Ok((
        pick_registry_for_package(&registries, &name, None),
        package_key.suffix.version().to_string(),
    ))
}

fn invalid_tarball_revision(
    package_key: &PackageKey,
    reason: &'static str,
) -> InstallPackageBySnapshotError {
    InstallPackageBySnapshotError::InvalidTarballRevision {
        package_key: package_key.to_string(),
        reason,
    }
}

/// Whether a tarball resolution that records no `integrity` may still
/// be fetched.
///
/// pnpm exempts the two shapes it never recorded a hash for, keyed off
/// the URL the way `classifyResolution` does — a lockfile's own
/// `gitHosted` marker is only a hint:
///
/// - a git-host archive URL, which pins a full commit SHA (older pnpm
///   versions wrote these without an `integrity`), and
/// - a `file:` tarball, which is local to the project.
///
/// Every other remote tarball must carry one, so bytes fetched over
/// the network for a package the lockfile claims to pin stay
/// verifiable.
#[must_use]
pub fn unverified_fetch_is_allowed(tarball_url: &str) -> bool {
    tarball_url.starts_with("file:") || is_git_hosted_tarball_url(tarball_url)
}

/// Build the host's [`PlatformSelector`] for runtime-variant
/// matching, from the host's os, cpu, and libc (the latter only on
/// Linux).
///
/// Translating `host_libc()`'s `"unknown"` to `None` lets
/// [`select_platform_variant`]'s asymmetric libc rule apply
/// consistently: `None` and `Some("glibc")` both require the
/// variant to omit `libc`, and `Some("musl")` requires an exact
/// match.
#[must_use]
pub fn host_platform_selector() -> PlatformSelector {
    let libc = match host_libc() {
        "unknown" => None,
        other => Some(other.to_string()),
    };
    PlatformSelector { os: host_platform().to_string(), cpu: host_arch().to_string(), libc }
}

/// Resolve the runtime archive selector from `supportedArchitectures`.
///
/// Exactly one archive is installed per runtime, so each axis prefers
/// the host's own value: an archive built for another platform cannot
/// run here.
///
/// <https://github.com/pnpm/pnpm/issues/13898>
#[must_use]
pub fn runtime_platform_selector(
    supported: Option<&pnpm_package_is_installable::SupportedArchitectures>,
) -> PlatformSelector {
    let host = host_platform_selector();
    let (requested_os, requested_cpu, requested_libc) = match supported {
        Some(supported) => {
            (supported.os.as_deref(), supported.cpu.as_deref(), supported.libc.as_deref())
        }
        None => (None, None, None),
    };
    PlatformSelector {
        os: pick_supported(requested_os, Some(&host.os)).unwrap_or(&host.os).to_string(),
        cpu: pick_supported(requested_cpu, Some(&host.cpu)).unwrap_or(&host.cpu).to_string(),
        libc: pick_supported(requested_libc, host.libc.as_deref()).map(str::to_string),
    }
}

fn pick_supported<'a>(
    requested: Option<&'a [String]>,
    host_value: Option<&'a str>,
) -> Option<&'a str> {
    let Some(requested) = requested.filter(|requested| !requested.is_empty()) else {
        return host_value;
    };
    if requested.iter().any(|value| value == "current" || Some(value.as_str()) == host_value) {
        return host_value;
    }
    requested.first().map(String::as_str)
}

/// Hand-coded matcher for the
/// `^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$)`
/// regex. Used as the archive-entry filter when extracting a Node.js
/// runtime archive: pnpm bundles `npm` + `corepack` in the tarball,
/// but pacquet (and pnpm) install pnpm itself as the package
/// manager, so the bundled tooling is dead weight and would also
/// shadow the user's pnpm via `node_modules/.bin/`. Stripping these
/// entries during the CAS write keeps the runtime artifact in the
/// store free of the bundled tooling without a post-hoc cleanup.
///
/// The hand-coded matcher avoids pulling a regex engine into
/// [`pnpm_tarball`].
fn node_extras_filter(path: &str) -> bool {
    bundled_tooling_module(path) || bundled_tooling_bin(path) || bundled_tooling_root_entry(path)
}

/// `^(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)`
fn bundled_tooling_module(path: &str) -> bool {
    let after_lib = path.strip_prefix("lib/").unwrap_or(path);
    let Some(rest) = after_lib.strip_prefix("node_modules/") else {
        return false;
    };
    ["npm", "corepack"].into_iter().any(|name| {
        rest.strip_prefix(name).is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
    })
}

/// `^bin/(?:npm|npx|corepack)$`
fn bundled_tooling_bin(path: &str) -> bool {
    path.strip_prefix("bin/").is_some_and(|rest| matches!(rest, "npm" | "npx" | "corepack"))
}

/// `^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$`
///
/// These are *not* under `bin/` — they live at the runtime archive root
/// after the `node-vX.Y.Z-<platform>-<arch>/` prefix strip.
fn bundled_tooling_root_entry(path: &str) -> bool {
    let stem = path.strip_suffix(".cmd").or_else(|| path.strip_suffix(".ps1")).unwrap_or(path);
    matches!(stem, "npm" | "npx" | "corepack")
}

/// Build the per-fetch [`IgnoreEntryFilter`] for the package being
/// installed.
///
/// The filter is cached in a [`std::sync::LazyLock`] so per-snapshot
/// `Arc::clone`s share one trait object — `IgnoreEntryFilter` is
/// a `dyn Fn`, so cheap to clone, and we don't want to allocate
/// the Arc once per runtime install.
fn archive_filter_for(package_key: &PackageKey) -> Option<Arc<IgnoreEntryFilter>> {
    if package_key.name.scope.is_some() || package_key.name.bare != "node" {
        return None;
    }
    static FILTER: std::sync::LazyLock<Arc<IgnoreEntryFilter>> = std::sync::LazyLock::new(|| {
        // `fn(&str) -> bool` implements `Fn(&str) -> bool + Send +
        // Sync`, so an `Arc<fn(...)>` unsizes to
        // `Arc<dyn Fn(...) + Send + Sync>` (the trait-object type
        // `IgnoreEntryFilter` aliases). The explicit type
        // annotation drives the unsizing coercion.
        let inner: Arc<IgnoreEntryFilter> = Arc::new(node_extras_filter);
        inner
    });
    Some(Arc::clone(&FILTER))
}

/// Fetch a [`BinaryResolution`] into the CAS, returning the
/// per-file `{relative_path → cas_path}` map the snapshot's virtual
/// directory needs. Dispatches on the archive type:
///
/// - [`BinaryArchive::Tarball`] uses [`IngestTarballToStore`]
///   with `package_unpacked_size: None` (binary archives don't
///   carry that hint).
/// - [`BinaryArchive::Zip`] uses [`IngestZipArchiveToStore`]
///   with `archive_prefix: binary.prefix.as_deref()` so the runtime
///   archive's top-level wrapper (e.g.
///   `node-v22.0.0-darwin-arm64/`) is stripped before the CAS keys
///   are written.
#[expect(
    clippy::too_many_arguments,
    reason = "matches the field set IngestTarballToStore / IngestZipArchiveToStore need"
)]
async fn fetch_binary_resolution_to_cas<Reporter: self::Reporter>(
    binary: &BinaryResolution,
    http_client: &ThrottledClient,
    config: &'static Config,
    store_index: Option<&SharedReadonlyStoreIndex>,
    store_index_writer: Option<&Arc<StoreIndexWriter>>,
    verified_files_cache: &SharedVerifiedFilesCache,
    prefetched_cas_paths: Option<&PrefetchedCasPaths>,
    package_key: &PackageKey,
    requester: &str,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
    let package_id = package_key.pkg_id();

    // Synthesize the `package.json` runtime archives (Node.js / Bun /
    // Deno) don't ship, and hand it to the fetcher as `append_manifest`.
    // The fetcher folds it into both this install's `cas_paths` and the
    // persisted store-index row (its `files` map and bundled `manifest`),
    // so a later *warm* install — which materializes straight from the
    // row and never re-runs this function — still lands a `package.json`
    // slot and lets the bin linker find the runtime's bin. The object
    // carries `name`, `version`, and `bin` — the three fields pacquet's
    // bin linking and `dlx` look at.
    let manifest_bytes = synthesize_runtime_manifest_bytes(package_key, binary)?;
    let cas_paths = match binary.archive {
        BinaryArchive::Tarball => IngestTarballToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
            verified_files_cache: Arc::clone(verified_files_cache),
            package_integrity: Some(&binary.integrity),
            package_unpacked_size: None,
            package_file_count: None,
            package_url: &binary.url,
            package_id: &package_id,
            requester,
            prefetched_cas_paths,
            retry_opts: retry_opts_from_config(config),
            auth_headers: &config.auth_headers,
            ignore_file_pattern,
            offline: config.offline,
            // Cold-batch binary tarball download: emits `fetched`
            // directly, so no network-fetched tracking is needed.
            progress_reported: None,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: Some(&manifest_bytes),
            },
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?,
        BinaryArchive::Zip => IngestZipArchiveToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
            verified_files_cache: Arc::clone(verified_files_cache),
            package_integrity: &binary.integrity,
            package_url: &binary.url,
            package_id: &package_id,
            requester,
            prefetched_cas_paths,
            retry_opts: retry_opts_from_config(config),
            auth_headers: &config.auth_headers,
            archive_prefix: binary.prefix.as_deref(),
            ignore_file_pattern,
            offline: config.offline,
            store_projection: pnpm_tarball::ArchiveStoreProjection::Package {
                append_manifest: Some(&manifest_bytes),
            },
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?,
    };

    Ok(cas_paths)
}

/// Serialize the synthesized runtime `package.json` to bytes.
///
/// `serde_json::to_vec` writes a single-line UTF-8 blob. The bytes go
/// straight into the CAS, where they're addressed by the SHA-512 of their
/// content; two runtime archives whose `(name, version, bin)`
/// triple happens to match share the same blob.
fn synthesize_runtime_manifest_bytes(
    package_key: &PackageKey,
    binary: &BinaryResolution,
) -> Result<Vec<u8>, InstallPackageBySnapshotError> {
    let bin_value = match &binary.bin {
        BinarySpec::Single(path) => serde_json::Value::String(path.clone()),
        BinarySpec::Map(map) => {
            let mut obj = serde_json::Map::with_capacity(map.len());
            for (name, path) in map {
                obj.insert(name.clone(), serde_json::Value::String(path.clone()));
            }
            serde_json::Value::Object(obj)
        }
    };
    let stripped = package_key.without_peer();
    let manifest = serde_json::json!({
        "name": stripped.name.to_string(),
        "version": stripped.suffix.version().to_string(),
        "bin": bin_value,
    });
    serde_json::to_vec(&manifest).map_err(|error| {
        InstallPackageBySnapshotError::SynthesizeRuntimeManifest {
            package_key: package_key.to_string(),
            error,
        }
    })
}

/// Render a variant's target list as a human-readable string for
/// inclusion in the [`InstallPackageBySnapshotError::NoMatchingPlatformVariant`]
/// error.
fn render_variant_targets(variants: &[pnpm_lockfile::PlatformAssetResolution]) -> String {
    let mut entries: Vec<String> = Vec::new();
    for variant in variants {
        for target in &variant.targets {
            match &target.libc {
                Some(libc) => entries.push(format!("{}/{}+{libc}", target.os, target.cpu)),
                None => entries.push(format!("{}/{}", target.os, target.cpu)),
            }
        }
    }
    entries.join(", ")
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
fn emit_progress_resolved<Reporter: self::Reporter>(package_id: &str, requester: &str) {
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Resolved {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

#[cfg(test)]
mod tests;
