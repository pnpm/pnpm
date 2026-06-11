use crate::{
    AllowBuildPolicy, CreateVirtualDirBySnapshot, CreateVirtualDirError, VirtualStoreLayout,
    retry_config::retry_opts_from_config,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::{Config, NodeLinker};
use pacquet_directory_fetcher::DirectoryFetcherError;
use pacquet_executor::ScriptsPrependNodePath as ExecScriptsPrependNodePath;
use pacquet_git_fetcher::{GitFetchOutput, GitFetcher, GitFetcherError, GitHostedTarballFetcher};
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PackageKey, PackageMetadata,
    PlatformSelector, SnapshotEntry, select_platform_variant,
};
use pacquet_network::ThrottledClient;
use pacquet_reporter::{LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter};
use pacquet_store_dir::{
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter,
    git_hosted_store_index_key,
};
use pacquet_tarball::{
    DownloadTarballToStore, DownloadZipArchiveToStore, IgnoreEntryFilter, MemCache,
    PrefetchedCasPaths, SharedReportedProgressKeys, TarballError,
};
use pipe_trait::Pipe;
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

/// This subroutine downloads a package tarball, extracts it, installs it to a
/// virtual dir, then creates the symlink layout for the package. CAS file
/// import and symlink creation run concurrently via `rayon::join` inside
/// [`CreateVirtualDirBySnapshot::run`].
#[must_use]
pub struct InstallPackageBySnapshot<'a> {
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    /// Install-scoped slot-directory mapping (GVS-aware). Drives the
    /// per-snapshot directory passed to
    /// [`CreateVirtualDirBySnapshot`] after the cold-batch download
    /// finishes. See [`crate::VirtualStoreLayout`].
    pub layout: &'a VirtualStoreLayout,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Install-scoped batched cache lookup result. See
    /// [`pacquet_tarball::prefetch_cas_paths`].
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Install-scoped shared in-flight tarball cache. When present, the
    /// registry/tarball download routes through
    /// [`DownloadTarballToStore::run_with_mem_cache`] so it parks on (or
    /// reuses) a download already in flight or completed for the same
    /// URL, rather than racing a second fetch of the same bytes. Both
    /// background prefetchers feed it: the pnpr client's
    /// [`crate::TarballPrefetcher`] (frozen materialization) and the
    /// fresh-resolve path's [`crate::PrefetchingResolver`] (cold batch,
    /// closing the race in
    /// <https://github.com/pnpm/pnpm/issues/12241>). `None` keeps the
    /// standalone `run_without_mem_cache` path for installs with no
    /// prefetcher (e.g. a plain `--frozen-lockfile` without pnpr).
    pub tarball_mem_cache: Option<&'a Arc<MemCache>>,
    /// Install-scoped package-status progress dedupe. Shared with the
    /// resolve-time prefetcher on the fresh path so the cold fallback
    /// does not double-count a package whose early prefetch already
    /// emitted `fetched` or `found_in_store`.
    pub progress_reported: Option<&'a SharedReportedProgressKeys>,
    /// Install-scoped `verifiedFilesCache` shared across every
    /// per-snapshot fetch. See `DownloadTarballToStore::verified_files_cache`
    /// for the rationale.
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter events (`pnpm:progress`'s
    /// `requester`). Same value as the `prefix` in
    /// [`pacquet_reporter::StageLog`].
    pub requester: &'a str,
    pub package_key: &'a PackageKey,
    pub metadata: &'a PackageMetadata,
    pub snapshot: &'a SnapshotEntry,
    /// `allowBuilds` gate. Routed into the git fetcher for
    /// `preparePackage`'s `GIT_DEP_PREPARE_NOT_ALLOWED` check.
    /// Computed once per install in
    /// [`crate::InstallFrozenLockfile::run`] and threaded through
    /// [`crate::CreateVirtualStore`].
    pub allow_build_policy: &'a AllowBuildPolicy,
    /// Workspace / lockfile root used to resolve directory-typed
    /// resolutions (`LockfileResolution::Directory`) against. Pnpm's
    /// directory-fetcher computes the source dir as
    /// `path.resolve(opts.lockfileDir, resolution.directory)`; pacquet
    /// threads the same value through so the resolved source matches
    /// upstream byte-for-byte even for relative resolutions like
    /// `../local-pkg`.
    pub workspace_root: &'a Path,
    /// Snapshots whose slots were not materialized on this host —
    /// threaded into [`CreateVirtualDirBySnapshot`] so the per-slot
    /// `create_symlink_layout` step can skip optional siblings whose
    /// target slot is absent (platform mismatch, `--no-optional`
    /// exclusion, or swallowed optional fetch failure). See
    /// [`crate::SkippedSnapshots`] for how it is built.
    pub skipped: &'a crate::SkippedSnapshots,
    /// Selects between the isolated and hoisted install layouts.
    /// `Isolated` runs [`CreateVirtualDirBySnapshot`] at the end of
    /// the per-snapshot fetch to populate the virtual-store slot;
    /// `Hoisted` skips that step because the hoisted linker
    /// ([`crate::link_hoisted_modules()`]) consumes the returned
    /// `cas_paths` directly and writes them into project-tree
    /// `node_modules/<alias>` directories. Either way the CAS files
    /// land in the store, so this is purely about whether the
    /// virtual-store slot gets materialized. Mirrors upstream's
    /// [`nodeLinker === 'hoisted'`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L411-L425)
    /// branch in `headlessInstall`.
    pub node_linker: NodeLinker,
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
    pub(crate) link_concurrency_probe:
        Option<&'a crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe>,
}

/// Error type of [`InstallPackageBySnapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallPackageBySnapshotError {
    #[diagnostic(transparent)]
    DownloadTarball(#[error(source)] TarballError),

    #[diagnostic(transparent)]
    CreateVirtualDir(#[error(source)] CreateVirtualDirError),

    #[display(
        "Package `{package_key}` has a tarball resolution without an `integrity` field; pacquet cannot verify the download and refuses to install it."
    )]
    #[diagnostic(code(pacquet_package_manager::missing_tarball_integrity))]
    MissingTarballIntegrity { package_key: String },

    #[display(
        "Package `{package_key}` uses a `{resolution_kind}` resolution, which pacquet does not yet support."
    )]
    #[diagnostic(code(pacquet_package_manager::unsupported_resolution))]
    UnsupportedResolution { package_key: String, resolution_kind: &'static str },

    /// Failure from either git fetcher: the git-CLI path for
    /// `type: git` resolutions (clone / checkout / preparePackage /
    /// CAS import) or the git-hosted-tarball post-pass for
    /// `TarballResolution { gitHosted: true }` (materialize /
    /// preparePackage / packlist / re-import). Both share the same
    /// `GitFetcherError` taxonomy because they share `prepare_package`,
    /// `packlist`, and the CAS-import helpers; the variant covers
    /// every fetcher path that exits through `pacquet-git-fetcher`.
    #[diagnostic(transparent)]
    GitFetch(#[error(source)] GitFetcherError),

    /// Failure from the directory fetcher: walking the source
    /// directory of an injected workspace dep, reading its manifest,
    /// or running the npm-packlist filter for
    /// `includeOnlyPackageFiles` mode. Mirrors the failure surface
    /// of pnpm's `directory-fetcher` at
    /// <https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts>.
    #[diagnostic(transparent)]
    DirectoryFetch(#[error(source)] DirectoryFetcherError),

    /// No variant in a [`LockfileResolution::Variations`] matches the
    /// host triple `(os, cpu, libc?)`. Surfaces with the host triple
    /// plus the list of advertised target triples so the user can see
    /// at a glance whether they're running on an unsupported platform
    /// or whether the lockfile was generated without the host's
    /// architecture in mind.
    #[display(
        "Package `{package_key}` is a runtime dependency, but none of its declared variants matches the host triple (os = `{host_os}`, cpu = `{host_cpu}`, libc = `{host_libc:?}`). Available variants: {available_targets}"
    )]
    #[diagnostic(code(pacquet_package_manager::no_matching_platform_variant))]
    NoMatchingPlatformVariant {
        package_key: String,
        host_os: &'static str,
        host_cpu: &'static str,
        host_libc: Option<&'static str>,
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
    /// Upstream contract guarantees variants are atomic
    /// `BinaryResolution`s; this variant catches lockfile corruption
    /// or a future shape pacquet doesn't recognise rather than
    /// silently routing through and confusing the install pipeline.
    #[display(
        "Package `{package_key}` carries a runtime variant whose inner resolution is `{inner_kind}` rather than `binary`; pacquet only knows how to install binary-shaped variants."
    )]
    #[diagnostic(code(pacquet_package_manager::variant_has_non_binary_resolution))]
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
    #[diagnostic(code(pacquet_package_manager::synthesize_runtime_manifest))]
    SynthesizeRuntimeManifest {
        package_key: String,
        #[error(source)]
        error: serde_json::Error,
    },
}

impl InstallPackageBySnapshot<'_> {
    /// Execute the subroutine. Returns the CAS file index for the
    /// fetched package — the map relative-archive-path →
    /// absolute-store-path that downstream consumers use to either
    /// populate a virtual-store slot (isolated) or import into a
    /// hoisted `node_modules/<alias>/` directly (hoisted).
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
        self,
    ) -> Result<HashMap<String, PathBuf>, InstallPackageBySnapshotError> {
        let InstallPackageBySnapshot {
            http_client,
            config,
            layout,
            store_index,
            store_index_writer,
            prefetched_cas_paths,
            tarball_mem_cache,
            progress_reported,
            verified_files_cache,
            logged_methods,
            requester,
            package_key,
            metadata,
            snapshot,
            allow_build_policy,
            skipped,
            workspace_root,
            node_linker,
            defer_link,
            #[cfg(test)]
            link_concurrency_probe,
        } = self;

        // TODO: skip when already exists in store?
        let package_id = package_key.without_peer().to_string();
        emit_progress_resolved::<Reporter>(&package_id, requester);

        // Adapter shared between the `Git` arm below and the
        // `gitHosted: true` post-pass on tarballs. Named local so
        // both fetchers can borrow it across their `.await` without
        // depending on temporary-lifetime extension.
        //
        // `AllowBuildPolicy::check` returns `None` when the package
        // is neither allow-listed nor deny-listed. Default-deny
        // (`None → false`) matches pnpm v11's policy: build scripts
        // have to be explicitly opted in to run.
        let allow_build_closure =
            |dep_path: &str| allow_build_policy.check(dep_path).unwrap_or(false);
        let scripts_prepend_node_path = match config.scripts_prepend_node_path {
            pacquet_config::ScriptsPrependNodePath::Always => ExecScriptsPrependNodePath::Always,
            pacquet_config::ScriptsPrependNodePath::Never => ExecScriptsPrependNodePath::Never,
            pacquet_config::ScriptsPrependNodePath::WarnOnly => {
                ExecScriptsPrependNodePath::WarnOnly
            }
        };

        let cas_paths: HashMap<String, PathBuf> = match &metadata.resolution {
            LockfileResolution::Tarball(_) | LockfileResolution::Registry(_) => {
                let (tarball_url, integrity) =
                    tarball_url_and_integrity(&metadata.resolution, package_key, config)?;
                let download = DownloadTarballToStore {
                    http_client,
                    store_dir: &config.store_dir,
                    store_index: store_index.cloned(),
                    store_index_writer: store_index_writer.cloned(),
                    verify_store_integrity: config.verify_store_integrity,
                    verified_files_cache: Arc::clone(verified_files_cache),
                    package_integrity: integrity,
                    package_unpacked_size: None,
                    package_file_count: None,
                    package_url: &tarball_url,
                    package_id: &package_id,
                    requester,
                    prefetched_cas_paths,
                    retry_opts: retry_opts_from_config(config),
                    auth_headers: &config.auth_headers,
                    ignore_file_pattern: None,
                    offline: config.offline,
                    progress_reported: progress_reported.cloned(),
                };
                // Reuse an in-flight or completed background download
                // through the shared mem cache when one is provided;
                // otherwise fetch standalone. The owned `HashMap` is
                // cloned out of the shared `Arc` so the rest of this pass
                // keeps its by-value contract.
                //
                // Restricted to registry resolutions: those are the only
                // ones the background prefetchers populate under a key
                // this pass also writes — the pnpr `TarballPrefetcher` and
                // the resolve-time `PrefetchingResolver` both key by
                // `name@version`, matching the materialization store-index
                // row. A remote tarball, by contrast, resolves with no
                // `name_ver`, so the prefetcher skips it; its only
                // mem-cache entry comes from the resolver's
                // download-to-resolve, keyed by `name@version`, whereas the
                // lockfile (and this pass) address it by `name@<url>`.
                // Reusing that entry would skip writing the `name@<url>`
                // store-index row a later re-resolve needs to reuse the
                // warm store, so remote tarballs must take the standalone
                // path. See <https://github.com/pnpm/pnpm/issues/12241>.
                let raw_cas_paths = match tarball_mem_cache {
                    Some(mem_cache)
                        if matches!(&metadata.resolution, LockfileResolution::Registry(_)) =>
                    {
                        // `clone()` is cheap (refs + `Arc`s) and lets us
                        // retry through `run_without_mem_cache` below if
                        // the shared download failed.
                        match download.clone().run_with_mem_cache::<Reporter>(mem_cache).await {
                            Ok(cas_paths) => Ok((*cas_paths).clone()),
                            // The prefetch is best-effort: if the sibling
                            // download for this URL failed (transient
                            // network, etc.), do our own retried fetch
                            // rather than inheriting the failure.
                            Err(TarballError::SiblingFetchFailed { .. }) => {
                                download.run_without_mem_cache::<Reporter>().await
                            }
                            Err(err) => Err(err),
                        }
                    }
                    _ => download.run_without_mem_cache::<Reporter>().await,
                }
                .map_err(InstallPackageBySnapshotError::DownloadTarball)?;

                // Run the git-hosted prepare+packlist pass for
                // tarballs sourced from a git host. Mirrors pnpm's
                // dispatch at `fetching/pick-fetcher/src/index.ts`:
                // a `gitHosted: true` tarball routes through
                // `gitHostedTarballFetcher` rather than the plain
                // `remoteTarballFetcher`, because the host's archive
                // endpoint doesn't run `prepare`/`prepublish*` and
                // the file set typically needs packlist filtering.
                if let LockfileResolution::Tarball(t) = &metadata.resolution
                    && t.git_hosted == Some(true)
                {
                    // `built = true` matches the dispatcher's default
                    // (`ignore_scripts: false` everywhere). When
                    // pacquet adds a configurable ignore-scripts mode
                    // this `true` flips to `!ignore_scripts`, in lock-
                    // step with the key shape `snapshot_cache_key`
                    // produces — otherwise the prefetch and the write
                    // would address different slots.
                    let files_index_file = git_hosted_store_index_key(&package_id, true);
                    let GitFetchOutput { cas_paths, built: _built } = GitHostedTarballFetcher {
                        cas_paths: raw_cas_paths,
                        path: t.path.as_deref(),
                        allow_build: &allow_build_closure,
                        ignore_scripts: false,
                        unsafe_perm: config.unsafe_perm,
                        user_agent: None,
                        scripts_prepend_node_path,
                        script_shell: None,
                        node_execpath: None,
                        npm_execpath: None,
                        store_dir: &config.store_dir,
                        package_id: &package_id,
                        requester,
                        store_index_writer,
                        files_index_file: &files_index_file,
                    }
                    .run::<Reporter>()
                    .await
                    .map_err(InstallPackageBySnapshotError::GitFetch)?;
                    cas_paths
                } else {
                    raw_cas_paths
                }
            }
            LockfileResolution::Directory(dir_resolution) => {
                // Injected workspace dep (`file:./local-pkg` with
                // `dependenciesMeta[*].injected = true`). Upstream's
                // [`directory-fetcher`](https://github.com/pnpm/pnpm/blob/85ceff2383/fetching/directory-fetcher/src/index.ts#L26-L32)
                // resolves the source dir as
                // `path.resolve(opts.lockfileDir, resolution.directory)`
                // and returns `local: true` with a `filesMap` that
                // points directly at the source files (no CAFS write).
                // Pacquet does the same: the `files_map` keys are the
                // forward-slash relative paths, the values are the
                // source paths, and downstream `link_file` /
                // `import_indexed_dir` hardlink-or-copy from those
                // source paths into the slot / hoisted directory just
                // like they would from a CAS-resident entry.
                //
                // `include_only_package_files = false` /
                // `resolve_symlinks = false` match upstream's defaults
                // in [`extendInstallOptions.ts:41`](https://github.com/pnpm/pnpm/blob/85ceff2383/installing/deps-installer/src/install/extendInstallOptions.ts#L41).
                // Wiring those through pacquet's config surface is a
                // follow-up; see the `resolveSymlinksInInjectedDirs`
                // / `includeOnlyPackageFiles` plumbing tracked in the
                // directory-fetcher PR description.
                let directory = workspace_root.join(&dir_resolution.directory);
                let output = pacquet_directory_fetcher::DirectoryFetcher {
                    directory,
                    include_only_package_files: false,
                    resolve_symlinks: false,
                }
                .run()
                .map_err(InstallPackageBySnapshotError::DirectoryFetch)?;
                output.files_map
            }
            // Slice A of <https://github.com/pnpm/pacquet/issues/437> wires the lockfile types; the install
            // dispatch for `Binary` / `Variations` lands in Slice D.
            // Until then, surface the kind via the typed
            // `UnsupportedResolution` error so a v11 lockfile with a
            // Runtime artifacts (Node.js / Bun / Deno) — `Binary`
            // and `Variations` carry a `BinaryResolution` describing
            // the archive to fetch. `Variations` is the multi-
            // platform wrapper: pick the variant whose `targets`
            // includes the host triple, then route through the same
            // `BinaryResolution` extractor (mirrors upstream's
            // [`binary-fetcher/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts)).
            LockfileResolution::Binary(binary) => {
                fetch_binary_resolution_to_cas::<Reporter>(
                    binary,
                    http_client,
                    config,
                    store_index,
                    store_index_writer,
                    verified_files_cache,
                    prefetched_cas_paths,
                    package_key,
                    requester,
                    archive_filter_for(package_key),
                )
                .await?
            }
            LockfileResolution::Variations(variations) => {
                let selector = host_platform_selector();
                let Some(variant) = select_platform_variant(&variations.variants, &selector) else {
                    return Err(InstallPackageBySnapshotError::NoMatchingPlatformVariant {
                        package_key: package_key.to_string(),
                        host_os: host_platform(),
                        host_cpu: host_arch(),
                        host_libc: match host_libc() {
                            "unknown" => None,
                            other => Some(other),
                        },
                        available_targets: render_variant_targets(&variations.variants),
                    });
                };
                // Upstream's `PlatformAssetResolution.resolution`
                // is always atomic (`BinaryResolution`); pacquet's
                // type widens to the full `LockfileResolution` for
                // serde uniformity but `select_platform_variant`'s
                // docs spell out that nested `Variations` would just
                // route their picked variant's inner shape back
                // through this dispatcher (no infinite recursion
                // because this arm doesn't call back into the
                // variant selector). The match below only
                // recognises `Binary`; anything else is either a
                // corrupt lockfile or a future shape pacquet hasn't
                // learned about yet, so reject loudly rather than
                // silently route through.
                let LockfileResolution::Binary(binary) = &variant.resolution else {
                    return Err(InstallPackageBySnapshotError::VariantHasNonBinaryResolution {
                        package_key: package_key.to_string(),
                        inner_kind: match &variant.resolution {
                            LockfileResolution::Tarball(_) => "tarball",
                            LockfileResolution::Registry(_) => "registry",
                            LockfileResolution::Directory(_) => "directory",
                            LockfileResolution::Git(_) => "git",
                            LockfileResolution::Variations(_) => "variations",
                            // Already matched above; reach is unreachable.
                            LockfileResolution::Binary(_) => "binary",
                        },
                    });
                };
                fetch_binary_resolution_to_cas::<Reporter>(
                    binary,
                    http_client,
                    config,
                    store_index,
                    store_index_writer,
                    verified_files_cache,
                    prefetched_cas_paths,
                    package_key,
                    requester,
                    archive_filter_for(package_key),
                )
                .await?
            }
            LockfileResolution::Git(git_resolution) => {
                // Same `built = true` rationale as the git-hosted
                // tarball branch above — key shape stays in lock-step
                // with `snapshot_cache_key`.
                let files_index_file = git_hosted_store_index_key(&package_id, true);
                let GitFetchOutput { cas_paths, built: _built } = GitFetcher {
                    repo: &git_resolution.repo,
                    commit: &git_resolution.commit,
                    path: git_resolution.path.as_deref(),
                    git_shallow_hosts: &config.git_shallow_hosts,
                    allow_build: &allow_build_closure,
                    ignore_scripts: false,
                    unsafe_perm: config.unsafe_perm,
                    user_agent: None,
                    scripts_prepend_node_path,
                    script_shell: None,
                    node_execpath: None,
                    npm_execpath: None,
                    store_dir: &config.store_dir,
                    package_id: &package_id,
                    requester,
                    store_index_writer,
                    files_index_file: &files_index_file,
                    git_bin: None,
                }
                .run::<Reporter>()
                .await
                .map_err(InstallPackageBySnapshotError::GitFetch)?;
                cas_paths
            }
        };

        // Under hoisted, the virtual-store slot would be unused —
        // [`crate::link_hoisted_modules()`] consumes the CAS paths
        // directly to materialize project-tree `node_modules/`
        // directories, so any slot we'd write here would only waste
        // disk. Mirrors upstream's branch at
        // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L411-L425>:
        // hoisted skips both `linkAllModules` (slot symlinks) and
        // `linkAllPkgs` (slot file imports), and runs
        // `linkHoistedModules` over the CAS paths instead.
        if !defer_link && matches!(node_linker, NodeLinker::Isolated | NodeLinker::Pnp) {
            CreateVirtualDirBySnapshot {
                layout,
                cas_paths: &cas_paths,
                import_method: config.package_import_method,
                logged_methods,
                requester,
                package_id: &package_id,
                package_key,
                snapshot,
                skipped,
                #[cfg(test)]
                link_concurrency_probe,
            }
            .run::<Reporter>()
            .map_err(InstallPackageBySnapshotError::CreateVirtualDir)?;
        }

        Ok(cas_paths)
    }
}

/// Resolve the tarball URL + integrity for tarball- and registry-shaped
/// resolutions. Factored out so the per-resolution-type dispatch in
/// [`InstallPackageBySnapshot::run`] reads top-down: each variant builds
/// its own `cas_paths`. Public because the pnpr server derives the same
/// URLs when it announces a verified frozen lockfile's tarballs to the
/// client — both sides must derive byte-identical URLs so the client's
/// prefetch mem-cache keys line up.
///
/// # Panics
///
/// On directory / git / binary / variations resolutions — callers gate
/// on the tarball/registry shapes first.
pub fn tarball_url_and_integrity<'a>(
    resolution: &'a LockfileResolution,
    package_key: &PackageKey,
    config: &'a Config,
) -> Result<(Cow<'a, str>, &'a ssri::Integrity), InstallPackageBySnapshotError> {
    match resolution {
        LockfileResolution::Tarball(tarball_resolution) => {
            let integrity = tarball_resolution.integrity.as_ref().ok_or_else(|| {
                InstallPackageBySnapshotError::MissingTarballIntegrity {
                    package_key: package_key.to_string(),
                }
            })?;
            Ok((tarball_resolution.tarball.as_str().pipe(Cow::Borrowed), integrity))
        }
        LockfileResolution::Registry(registry_resolution) => {
            let registry = config.registry.strip_suffix('/').unwrap_or(&config.registry);
            let name = &package_key.name;
            let version = package_key.suffix.version();
            let bare_name = name.bare.as_str();
            let tarball_url = format!("{registry}/{name}/-/{bare_name}-{version}.tgz");
            Ok((Cow::Owned(tarball_url), &registry_resolution.integrity))
        }
        // Caller (`run`) only invokes this helper for the tarball /
        // registry arms; git, directory, binary, and variations
        // resolutions never reach here. Return an unreachable-style
        // error so a future caller that forgets to gate gets a
        // clear panic in debug.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_) => {
            unreachable!("tarball_url_and_integrity called with non-tarball resolution");
        }
    }
}

/// Build the host's [`PlatformSelector`] for runtime-variant
/// matching. Mirrors pnpm's call shape at the binary-fetcher
/// dispatch site: `{ os: process.platform, cpu: process.arch, libc:
/// process.platform === 'linux' ? family : null }`.
///
/// `host_libc()` returns `"unknown"` on every non-Linux host and
/// `"glibc"` / `"musl"` on Linux. Translate `"unknown"` to `None`
/// so [`select_platform_variant`]'s asymmetric libc rule applies
/// the same way upstream's does: `None` and `Some("glibc")` both
/// require the variant to omit `libc`, and `Some("musl")` requires
/// an exact match.
pub(crate) fn host_platform_selector() -> PlatformSelector {
    let libc = match host_libc() {
        "unknown" => None,
        other => Some(other.to_string()),
    };
    PlatformSelector { os: host_platform().to_string(), cpu: host_arch().to_string(), libc }
}

/// Hand-coded port of upstream's
/// [`NODE_EXTRAS_IGNORE_PATTERN`](https://github.com/pnpm/pnpm/blob/94240bc046/engine/runtime/node-resolver/src/index.ts)
/// regex (`^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$)`).
/// Used as the archive-entry filter when extracting a Node.js
/// runtime archive: pnpm bundles `npm` + `corepack` in the tarball,
/// but pacquet (and pnpm) install pnpm itself as the package
/// manager, so the bundled tooling is dead weight and would also
/// shadow the user's pnpm via `node_modules/.bin/`. Stripping these
/// entries during the CAS write keeps the runtime artifact in the
/// store free of the bundled tooling without a post-hoc cleanup.
///
/// Pacquet uses a hand-coded matcher rather than the upstream regex
/// so [`pacquet_tarball`] doesn't have to pull in a regex engine.
/// The three branches below mirror the regex alternation exactly;
/// every path the regex matches is matched here, and nothing else.
fn node_extras_filter(path: &str) -> bool {
    // ^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$))
    //
    // Strip an optional leading `lib/` so the `lib/node_modules/...`
    // and `node_modules/...` shapes converge into one check; the
    // `node_modules/` prefix is mandatory after the optional `lib/`.
    let after_lib = path.strip_prefix("lib/").unwrap_or(path);
    if let Some(rest) = after_lib.strip_prefix("node_modules/") {
        for name in ["npm", "corepack"] {
            if rest == name || rest.starts_with(&format!("{name}/")) {
                return true;
            }
        }
    }
    // ^bin/(?:npm|npx|corepack)$
    //
    // The `$` anchors the regex to an exact match — `bin/npm/foo`
    // doesn't trip this arm (and the `node_modules` arm above
    // wouldn't catch it either since it doesn't start with `bin/`).
    if let Some(rest) = path.strip_prefix("bin/")
        && matches!(rest, "npm" | "npx" | "corepack")
    {
        return true;
    }
    // ^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$
    //
    // Top-level shim files; `.cmd` / `.ps1` cover Windows. Note
    // these are *not* under `bin/` — they live at the runtime
    // archive root after the `node-vX.Y.Z-<platform>-<arch>/`
    // prefix strip.
    for name in ["npm", "npx", "corepack"] {
        if path == name {
            return true;
        }
        for ext in [".cmd", ".ps1"] {
            if path.len() == name.len() + ext.len() && path.starts_with(name) && path.ends_with(ext)
            {
                return true;
            }
        }
    }
    false
}

/// Build the per-fetch [`IgnoreEntryFilter`] for the package being
/// installed. Returns `Some(NODE_EXTRAS_IGNORE_PATTERN)` for
/// unscoped `node` (matching upstream's
/// [`archiveFilters: { node: NODE_EXTRAS_IGNORE_PATTERN }`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/client/src/index.ts)
/// keyed by `pkg.name`); everything else returns `None` and the
/// full archive contents land in the CAS unfiltered.
///
/// The filter is cached in a [`std::sync::OnceLock`] so per-snapshot
/// `Arc::clone`s share one trait object — `IgnoreEntryFilter` is
/// a `dyn Fn`, so cheap to clone, and we don't want to allocate
/// the Arc once per runtime install.
fn archive_filter_for(package_key: &PackageKey) -> Option<Arc<IgnoreEntryFilter>> {
    if package_key.name.scope.is_some() || package_key.name.bare != "node" {
        return None;
    }
    static FILTER: std::sync::OnceLock<Arc<IgnoreEntryFilter>> = std::sync::OnceLock::new();
    let filter = FILTER.get_or_init(|| {
        // `fn(&str) -> bool` implements `Fn(&str) -> bool + Send +
        // Sync`, so an `Arc<fn(...)>` unsizes to
        // `Arc<dyn Fn(...) + Send + Sync>` (the trait-object type
        // `IgnoreEntryFilter` aliases). The explicit type
        // annotation drives the unsizing coercion.
        let inner: Arc<IgnoreEntryFilter> = Arc::new(node_extras_filter);
        inner
    });
    Some(Arc::clone(filter))
}

/// Fetch a [`BinaryResolution`] into the CAS, returning the
/// per-file `{relative_path → cas_path}` map the snapshot's virtual
/// directory needs. Dispatches on the archive type:
///
/// - [`BinaryArchive::Tarball`] uses [`DownloadTarballToStore`]
///   with `package_unpacked_size: None` (binary archives don't
///   carry that hint upstream either).
/// - [`BinaryArchive::Zip`] uses [`DownloadZipArchiveToStore`]
///   with `archive_prefix: binary.prefix.as_deref()` so the runtime
///   archive's top-level wrapper (e.g.
///   `node-v22.0.0-darwin-arm64/`) is stripped before the CAS keys
///   are written.
///
/// The Node-runtime `NODE_EXTRAS_IGNORE_PATTERN` filter that strips
/// bundled `npm` / `corepack` from the archive will land in Slice
/// D2; for now the filter slot stays `None` and the full archive
/// contents are imported. Bin-link cmd-shims for the runtime
/// executables likewise wait for Slice D2.
#[expect(
    clippy::too_many_arguments,
    reason = "matches the field set DownloadTarballToStore / DownloadZipArchiveToStore need"
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
    let package_id = package_key.without_peer().to_string();
    let mut cas_paths = match binary.archive {
        BinaryArchive::Tarball => DownloadTarballToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
            verified_files_cache: Arc::clone(verified_files_cache),
            package_integrity: &binary.integrity,
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
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?,
        BinaryArchive::Zip => DownloadZipArchiveToStore {
            http_client,
            store_dir: &config.store_dir,
            store_index: store_index.cloned(),
            store_index_writer: store_index_writer.cloned(),
            verify_store_integrity: config.verify_store_integrity,
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
        }
        .run_without_mem_cache::<Reporter>()
        .await
        .map_err(InstallPackageBySnapshotError::DownloadTarball)?,
    };

    // Synthesize the package.json for the runtime archive and import
    // it through the same CAS write path the rest of the archive
    // takes. Runtime archives don't ship their own `package.json`,
    // so the existing bin-link step (which reads the manifest off
    // the slot's `package.json`) has nothing to consume by default.
    // Mirrors upstream's `appendManifest` flow at
    // [`binary-fetcher/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts):
    // the synthesized object has `name`, `version`, and `bin` — the
    // three fields pacquet's `link_bins_of_packages` actually looks
    // at. Writing the bytes through `write_cas_file` keeps the
    // import path uniform with every other CAS-imported file and
    // means the bytes are content-addressed (so two runtimes with
    // the same `(name, version, bin)` share one blob).
    let manifest_bytes = synthesize_runtime_manifest_bytes(package_key, binary)?;
    let (cas_path, _hash) =
        config.store_dir.write_cas_file(&manifest_bytes, false).map_err(|err| {
            InstallPackageBySnapshotError::DownloadTarball(TarballError::WriteCasFile(err))
        })?;
    if let Some(previous) = cas_paths.insert("package.json".to_string(), cas_path) {
        tracing::warn!(
            ?previous,
            ?package_id,
            "synthesized package.json displaced an existing entry — runtime archives are not expected to ship a package.json",
        );
    }
    Ok(cas_paths)
}

/// Serialize the synthesized runtime `package.json` to bytes. Three
/// fields, matching the upstream `appendManifest` shape:
///
/// - `name` — the package key's display form, scope-aware.
/// - `version` — the bare semver string from the peer-stripped key.
/// - `bin` — the lockfile-declared bins ([`BinarySpec`]). `Single`
///   becomes a JSON string (pnpm's convention: one binary, named
///   after the package), `Map` becomes a JSON object.
///
/// `serde_json::to_vec` writes a single-line UTF-8 blob — same
/// format upstream's worker thread emits. The bytes go straight
/// into the CAS, where they're addressed by the SHA-512 of their
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
/// error. Each target is rendered as `os/cpu` or `os/cpu+libc`,
/// joined with `, `.
fn render_variant_targets(variants: &[pacquet_lockfile::PlatformAssetResolution]) -> String {
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
/// cold-batch path is about to fetch. Mirrors pnpm's emit at
/// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-resolver/src/resolveDependencies.ts#L1586>:
/// one event per (resolved) package, fired before the fetch
/// attempt. In pacquet's frozen-lockfile path the lockfile *is* the
/// resolution, so each snapshot is "already resolved" by the time
/// we reach this site.
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
