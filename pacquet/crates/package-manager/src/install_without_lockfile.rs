use crate::{
    HoistedDependencies, InstallPackageFromRegistry, InstallPackageFromRegistryError,
    LinkVirtualStoreBins, LinkVirtualStoreBinsError, store_init::init_store_dir_best_effort,
};
use async_recursion::async_recursion;
use dashmap::DashSet;
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_cmd_shim::{LinkBinsError, RealApi, link_bins};
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::PackageVersion;
use pacquet_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter};
use pacquet_tarball::MemCache;
use pipe_trait::Pipe;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU8;

/// In-memory cache for packages that have started resolving dependencies.
///
/// The contents of set is the package's virtual_store_name.
/// e.g. `@pnpm.e2e/dep-1@1.0.0` →  `@pnpm.e2e+dep-1@1.0.0`
pub type ResolvedPackages = DashSet<String>;

/// This subroutine install packages from a `package.json` without reading or writing a lockfile.
///
/// **Brief overview for each package:**
/// * Fetch a tarball of the package.
/// * Extract the tarball into the store directory.
/// * Import (by reflink, hardlink, or copy) the files from the store dir to `node_modules/.pacquet/{name}@{version}/node_modules/{name}/`.
/// * Create dependency symbolic links in `node_modules/.pacquet/{name}@{version}/node_modules/`.
/// * Create a symbolic link at `node_modules/{name}`.
/// * Repeat the process for the dependencies of the package.
#[must_use]
pub struct InstallWithoutLockfile<'a, DependencyGroupList> {
    pub tarball_mem_cache: &'a MemCache,
    pub resolved_packages: &'a ResolvedPackages,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub manifest: &'a PackageManifest,
    pub dependency_groups: DependencyGroupList,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter `requester` fields.
    pub requester: &'a str,
}

/// Error type of [`InstallWithoutLockfile`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallWithoutLockfileError {
    #[diagnostic(transparent)]
    InstallPackageFromRegistry(#[error(source)] InstallPackageFromRegistryError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),

    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] LinkVirtualStoreBinsError),
}

impl<'a, DependencyGroupList> InstallWithoutLockfile<'a, DependencyGroupList> {
    /// Execute the subroutine.
    ///
    /// The without-lockfile path always returns an empty
    /// [`HoistedDependencies`] map. Hoisting needs the resolved
    /// snapshot graph the lockfile carries; without it, pacquet has
    /// nothing to walk. Frozen-lockfile installs (the production
    /// pacquet path) get the full hoist treatment via
    /// [`crate::InstallFrozenLockfile::run`]. The signature symmetry
    /// keeps `Install::run` from branching on which sub-path produced
    /// the result.
    pub async fn run<Reporter: self::Reporter>(
        self,
    ) -> Result<HoistedDependencies, InstallWithoutLockfileError>
    where
        DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    {
        let InstallWithoutLockfile {
            tarball_mem_cache,
            http_client,
            config,
            manifest,
            dependency_groups,
            resolved_packages,
            logged_methods,
            requester,
        } = self;

        let store_dir: &'static _ = &config.store_dir;

        // Eagerly create `files/00..ff` under the v11 store root so per-
        // tarball CAFS writes never pay a `create_dir_all` syscall on the
        // hot path. Ports pnpm's `initStore` in `worker/src/start.ts`.
        // See [`init_store_dir_best_effort`] for the error-degradation
        // policy shared with `create_virtual_store.rs`.
        init_store_dir_best_effort(store_dir).await;

        // Open the read-only SQLite index once per install, shared across
        // every `DownloadTarballToStore`. See the matching comment in
        // `create_virtual_store.rs` for the full rationale, including the
        // `JoinError`-to-cache-miss degradation (with a `warn!` so it
        // stays diagnosable).
        let store_index =
            match tokio::task::spawn_blocking(move || StoreIndex::shared_readonly_in(store_dir))
                .await
            {
                Ok(store_index) => store_index,
                Err(error) => {
                    tracing::warn!(
                        target: "pacquet::install",
                        ?error,
                        "store-index open task failed; continuing without a shared cache index",
                    );
                    None
                }
            };
        let store_index_ref = store_index.as_ref();

        // Batched store-index writer. See `create_virtual_store.rs` for
        // the full rationale — we spawn once, every tarball just queues a
        // row, and one writer task flushes them in batched transactions.
        let (store_index_writer, writer_task) = StoreIndexWriter::spawn(store_dir);
        let store_index_writer_ref = Some(&store_index_writer);

        // Install-scoped `verifiedFilesCache`. See the matching block
        // in `create_virtual_store.rs` for the full rationale — pnpm
        // threads one `Set<string>` through every package's verify
        // pass so a CAFS path stat'd for one package skips the stat
        // for any later package referencing the same blob.
        let verified_files_cache = SharedVerifiedFilesCache::default();

        manifest
            .dependencies(dependency_groups)
            .map(|(name, version_range)| {
                // Same pattern as `create_virtual_store.rs`: clone the
                // shared cache handle so each per-dependency future owns
                // a handle it can move into the `async move` block and
                // then reference from within the future.
                let verified_files_cache = SharedVerifiedFilesCache::clone(&verified_files_cache);
                async move {
                    let dependency = InstallPackageFromRegistry {
                        tarball_mem_cache,
                        http_client,
                        config,
                        store_index: store_index_ref,
                        store_index_writer: store_index_writer_ref,
                        verified_files_cache: &verified_files_cache,
                        logged_methods,
                        requester,
                        node_modules_dir: &config.modules_dir,
                        name,
                        version_range,
                    }
                    .run::<Reporter>()
                    .await
                    .map_err(InstallWithoutLockfileError::InstallPackageFromRegistry)?;

                    InstallWithoutLockfile {
                        tarball_mem_cache,
                        http_client,
                        config,
                        manifest,
                        dependency_groups: (),
                        resolved_packages,
                        logged_methods,
                        requester,
                    }
                    .install_dependencies_from_registry::<Reporter>(
                        &dependency,
                        store_index_ref,
                        store_index_writer_ref,
                        &verified_files_cache,
                    )
                    .await?;

                    Ok::<_, InstallWithoutLockfileError>(())
                }
            })
            .pipe(future::try_join_all)
            .await?;

        // Drop the orchestration's writer handle so the channel closes,
        // then wait for the final batch flush. See `create_virtual_store.rs`
        // for why errors here are downgraded to `warn!`.
        drop(store_index_writer);
        match writer_task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-index writer task returned an error; some rows may not be persisted",
            ),
            Err(error) => tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-index writer task panicked; some rows may not be persisted",
            ),
        }

        // Link bins. Direct dependencies first (root project's
        // `node_modules/.bin`) and then per-slot children inside the
        // virtual store. Mirrors the same two-call shape as
        // `install_frozen_lockfile.rs`. We re-walk `<modules_dir>` instead
        // of replaying the manifest because the `dependency_groups`
        // iterator was already consumed by the install loop above; pnpm's
        // own `linkBins(modulesDir, binsDir)` overload uses the same
        // strategy.
        link_bins::<RealApi>(&config.modules_dir, &config.modules_dir.join(".bin"))
            .map_err(InstallWithoutLockfileError::LinkBins)?;

        // No lockfile here, so no prefetched manifests are available —
        // fall back to the legacy readdir-driven path (slots discovered
        // by walking `<virtual_store_dir>`, child manifests read from
        // disk). The frozen-lockfile path skips both via
        // [`LinkVirtualStoreBins::snapshots`] / `package_manifests`.
        //
        // The bin linker also doesn't need GVS-aware slot lookups
        // here: without snapshots there are no GVS slot directories to
        // compute. Construct a legacy layout so the readdir path
        // enumerates `config.virtual_store_dir` exactly as before. GVS
        // is scoped to frozen-lockfile installs (pnpm/pacquet#432); the
        // without-lockfile fallback stays project-local.
        let layout = crate::VirtualStoreLayout::legacy(config.virtual_store_dir.clone());
        let empty_manifests = std::collections::HashMap::new();
        let empty_skipped = crate::SkippedSnapshots::new();
        LinkVirtualStoreBins {
            layout: &layout,
            snapshots: None,
            packages: None,
            package_manifests: &empty_manifests,
            // The without-lockfile path has no installability check
            // (no `packages:` metadata to evaluate constraints
            // against), so the skip set is empty by definition.
            skipped: &empty_skipped,
        }
        .run()
        .map_err(InstallWithoutLockfileError::LinkVirtualStoreBins)?;

        // Mirrors upstream `link.ts:167-170`: `importing_done` fires once
        // extraction and symlink linking are complete. The without-lockfile
        // path does not run lifecycle scripts today, so emitting here also
        // marks end-of-install for reporters.
        // <https://github.com/pnpm/pnpm/blob/80037699fb/installing/deps-installer/src/install/link.ts#L167>
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: requester.to_string(),
            stage: Stage::ImportingDone,
        }));

        Ok(BTreeMap::new())
    }
}

impl<'a> InstallWithoutLockfile<'a, ()> {
    /// Install dependencies of a dependency.
    #[async_recursion]
    async fn install_dependencies_from_registry<Reporter>(
        &self,
        package: &PackageVersion,
        store_index: Option<&'async_recursion pacquet_store_dir::SharedReadonlyStoreIndex>,
        store_index_writer: Option<
            &'async_recursion std::sync::Arc<pacquet_store_dir::StoreIndexWriter>,
        >,
        verified_files_cache: &'async_recursion SharedVerifiedFilesCache,
    ) -> Result<(), InstallWithoutLockfileError>
    where
        Reporter: self::Reporter,
    {
        let InstallWithoutLockfile {
            tarball_mem_cache,
            http_client,
            config,
            resolved_packages,
            ..
        } = self;

        // This package has already resolved, there is no need to reinstall again.
        if !resolved_packages.insert(package.to_virtual_store_name()) {
            tracing::info!(target: "pacquet::install", package = ?package.to_virtual_store_name(), "Skip subset");
            return Ok(());
        }

        let node_modules_path = self
            .config
            .virtual_store_dir
            .join(package.to_virtual_store_name())
            .join("node_modules");

        tracing::info!(target: "pacquet::install", node_modules = ?node_modules_path, "Start subset");

        let node_modules_path_ref = &node_modules_path;
        package
            .dependencies(self.config.auto_install_peers)
            .map(|(name, version_range)| async move {
                let dependency = InstallPackageFromRegistry {
                    tarball_mem_cache,
                    http_client,
                    config,
                    store_index,
                    store_index_writer,
                    verified_files_cache,
                    logged_methods: self.logged_methods,
                    requester: self.requester,
                    node_modules_dir: node_modules_path_ref,
                    name,
                    version_range,
                }
                .run::<Reporter>()
                .await
                .map_err(InstallWithoutLockfileError::InstallPackageFromRegistry)?;
                self.install_dependencies_from_registry::<Reporter>(
                    &dependency,
                    store_index,
                    store_index_writer,
                    verified_files_cache,
                )
                .await?;
                Ok::<_, InstallWithoutLockfileError>(())
            })
            .pipe(future::try_join_all)
            .await?;

        tracing::info!(target: "pacquet::install", node_modules = ?node_modules_path, "Complete subset");

        Ok(())
    }
}
