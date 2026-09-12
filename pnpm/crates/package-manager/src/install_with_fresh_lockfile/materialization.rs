use super::{
    FinalScope, FreshInputs, FreshPlan, HostProbeInputs, InstallShape,
    InstallWithFreshLockfileResult, LockfileOnlyOptions, LockfileViews, MaterializationScope,
    OnDiskInputs, OnDiskOutput, OwnedInputs, PlanLockfiles, PlanScope, Resolved, ResolverSetup,
    build_lockfile_phase, errors::InstallWithFreshLockfileError, finish_early_materialization,
    finish_lockfile_only, persist_fresh_lockfile, plan_fresh_materialization, resolver_setup,
    run_on_disk_phases, warn_stale_convergence_overrides_if_any,
};
use crate::{AllowBuildPolicy, VirtualStoreLayout};
use pnpm_catalogs_types::Catalogs;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pnpm_tarball::MemCache;
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct MaterializationResources {
    tarball_mem_cache: Arc<MemCache>,
    lockfile_specifier_manifests: Option<BTreeMap<String, PackageManifest>>,
    catalogs: Catalogs,
    node_version: Option<String>,
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    lockfile_verification_gate: Option<crate::LockfileVerificationGate>,
}
pub(super) struct MaterializationStores {
    index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
    writer: Option<Arc<pnpm_store_dir::StoreIndexWriter>>,
    writer_task: tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>,
    caches: resolver_setup::StoreCaches,
}
impl MaterializationStores {
    fn take_writer(&mut self) -> Arc<pnpm_store_dir::StoreIndexWriter> {
        self.writer.take().expect("store writer is available before materialization")
    }
}
impl From<resolver_setup::StoreIndexHandles> for MaterializationStores {
    fn from(stores: resolver_setup::StoreIndexHandles) -> Self {
        Self {
            index: stores.index,
            writer: Some(stores.writer),
            writer_task: stores.writer_task,
            caches: stores.caches,
        }
    }
}
pub(super) struct FreshMaterialization<'a, Reporter> {
    install: FreshInputs<'a>,
    resources: MaterializationResources,
    shape: InstallShape,
    stores: MaterializationStores,
    custom_fetcher_session: Option<Arc<pnpm_deps_restorer::CustomFetcherSession>>,
    resolved: Resolved<'a, Reporter>,
}
/// Release resolution caches before materializing. Boxing this phase bounds the
/// caller's future size and keeps the nested install future's `Send` proof local.
pub(super) fn finish_resolved_install<'a, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    owned: OwnedInputs,
    setup: ResolverSetup,
    resolved: Resolved<'a, Reporter>,
) -> futures_util::future::BoxFuture<
    'a,
    Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError>,
> {
    Box::pin(async move {
        warn_stale_overrides::<Reporter>(install, &setup, &resolved).await;
        // Resolution is over: release the resolvers and the metadata
        // cache before materializing, so their memory does not outlive
        // its use. The custom fetchers stay, the cold batch consults them.
        drop(setup.chain.resolver);
        drop(setup.chain.npm_resolver);
        drop(owned.meta_cache);
        drop(setup.chain.fetch_locker);
        drop(setup.chain.picked_manifest_cache);
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: install.lockfile_dir.display().to_string(),
            stage: Stage::ResolutionDone,
        }));
        FreshMaterialization {
            install,
            resources: MaterializationResources {
                tarball_mem_cache: owned.tarball_mem_cache,
                lockfile_specifier_manifests: owned.lockfile_specifier_manifests,
                catalogs: owned.catalogs,
                node_version: owned.node_version,
                early_host_detection: owned.early_host_detection,
                deps_requiring_build_sink: owned.deps_requiring_build_sink,
                lockfile_verification_gate: owned.lockfile_verification_gate,
            },
            shape: setup.shape,
            stores: setup.stores.into(),
            custom_fetcher_session: setup.chain.custom_fetcher_session,
            resolved,
        }
        .run()
        .await
    })
}
pub(super) async fn warn_stale_overrides<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    setup: &ResolverSetup,
    resolved: &Resolved<'_, Reporter>,
) {
    if resolved.full_resolution {
        warn_stale_convergence_overrides_if_any::<Reporter>(
            &*setup.chain.npm_resolver,
            resolved.parsed_overrides.as_deref(),
            resolved.versions_overrider.as_deref(),
            install.lockfile_dir,
            (setup.policy.published_by, setup.policy.published_by_exclude.as_ref()),
        )
        .await;
    }
}
impl<Reporter: self::Reporter + 'static> FreshMaterialization<'_, Reporter> {
    async fn run(
        mut self,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
        let allow_build_policy = (!self.install.lockfile_only)
            .then(|| AllowBuildPolicy::from_config(self.install.config))
            .transpose()
            .map_err(InstallWithFreshLockfileError::AllowBuildsPolicy)?;
        let built_lockfile = build_lockfile_phase::<Reporter>(
            self.install,
            &mut self.resources.lockfile_verification_gate,
            std::mem::take(&mut self.resolved.time),
            &self.resolved,
            LockfileViews {
                importer_manifests: &self.resolved.importer_manifests,
                wanted_lockfile: self
                    .resolved
                    .fixed_wanted_lockfile
                    .as_ref()
                    .or(self.install.wanted_lockfile),
                catalogs: &self.resources.catalogs,
                lockfile_specifier_manifests: self.resources.lockfile_specifier_manifests.as_ref(),
            },
            self.shape.verify_filtered_repair,
        )
        .await?;
        match allow_build_policy {
            Some(policy) => self.materialize(built_lockfile, policy).await,
            None => self.finish_lockfile_only(built_lockfile).await,
        }
    }

    async fn finish_lockfile_only(
        self,
        built_lockfile: Lockfile,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
        finish_lockfile_only::<Reporter>(LockfileOnlyOptions {
            built_lockfile,
            peer_issue_importer_ids: self.resolved.peer_issue_importer_ids,
            config: self.install.config,
            lockfile_dir: self.install.lockfile_dir,
            requester: self.install.requester,
            dry_run: self.install.dry_run,
            save_lockfile: self.install.save_lockfile,
            after_all_resolved_hook: self.resolved.after_all_resolved_hook.as_ref(),
            after_all_resolved_log: self.resolved.after_all_resolved_log,
            store_index_writer: self
                .stores
                .writer
                .expect("store writer is available before materialization"),
            writer_task: self.stores.writer_task,
        })
        .await
    }

    async fn materialize(
        mut self,
        built_lockfile: Lockfile,
        allow_build_policy: AllowBuildPolicy,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
        let initial =
            MaterializationScope::initial(self.install, self.shape.is_hoisted, &built_lockfile);
        let mut plan = plan_fresh_materialization::<Reporter>(
            self.install,
            HostProbeInputs {
                early_host_detection: self.resources.early_host_detection.take(),
                node_version: self.resources.node_version.take(),
            },
            PlanLockfiles { initial: initial.lockfile(&built_lockfile), built: &built_lockfile },
            &allow_build_policy,
            PlanScope {
                included: self.install.included(),
                include_transitive_optional_dependencies: self
                    .shape
                    .include_transitive_optional_dependencies,
            },
        )
        .await?;
        let scope =
            initial.finalize(self.install, self.shape.is_hoisted, &built_lockfile, &plan.skipped);
        finish_early_materialization(
            self.resolved.early_materializer.as_deref(),
            initial.lockfile(&built_lockfile).snapshots.as_ref(),
            &plan.skipped,
            self.install.logged_methods,
        )
        .await;
        let on_disk =
            self.run_on_disk(&built_lockfile, &scope, &mut plan, &allow_build_policy).await?;
        self.persist_result(built_lockfile, on_disk).await
    }

    async fn persist_result(
        self,
        built_lockfile: Lockfile,
        on_disk: OnDiskOutput,
    ) -> Result<InstallWithFreshLockfileResult, InstallWithFreshLockfileError> {
        let persisted = persist_fresh_lockfile(
            built_lockfile,
            self.install.config,
            self.install.lockfile_dir,
            self.install.save_lockfile,
            (
                self.resolved.after_all_resolved_hook.as_ref(),
                self.resolved.after_all_resolved_log.clone(),
            ),
        )
        .await?;
        Ok(InstallWithFreshLockfileResult {
            hoisted_dependencies: on_disk.hoisted_dependencies,
            hoisted_locations: on_disk.hoisted_locations,
            injected_deps: on_disk.injected_deps,
            peer_issue_importer_ids: self.resolved.peer_issue_importer_ids,
            wanted_lockfile: persisted.lockfile,
            can_record_lockfile_verification: persisted.can_record_lockfile_verification,
            ignored_builds: on_disk.ignored_builds,
            deferred_builds: on_disk.deferred_builds,
            skipped: on_disk.skipped,
            store_index_teardown: self.stores.writer_task,
        })
    }

    async fn run_on_disk(
        &mut self,
        built_lockfile: &Lockfile,
        scope: &FinalScope,
        plan: &mut FreshPlan<'_>,
        allow_build_policy: &AllowBuildPolicy,
    ) -> Result<OnDiskOutput, InstallWithFreshLockfileError> {
        let store_index_writer = self.stores.take_writer();
        run_on_disk_phases::<Reporter>(
            OnDiskInputs {
                install: self.install,
                ctx: &fresh_install_context(
                    self.install,
                    &self.shape,
                    &self.stores.caches,
                    &plan.layout,
                    allow_build_policy,
                ),
                include_transitive_optional_dependencies: self
                    .shape
                    .include_transitive_optional_dependencies,
                deps_requiring_build_sink: self.resources.deps_requiring_build_sink.take(),
                tarball_mem_cache: &self.resources.tarball_mem_cache,
                materialization_lockfile: scope.lockfile(built_lockfile),
                importer_manifests: &self.resolved.importer_manifests,
                project_anchor_importer_ids: &scope.project_anchor_importer_ids,
                dir_clone_cache: plan.dir_clone_cache.as_ref(),
                host_node: plan.host_node.as_ref(),
                engine_name: plan.engine_name.take(),
                deferred_engine_name: plan.deferred_engine_name.take(),
                patched_dependencies: self.resolved.patched_dependencies.as_deref(),
                custom_fetcher_session: self.custom_fetcher_session.as_ref(),
                store_index_ref: self.stores.index.as_ref(),
                store_index_writer,
                caches: &self.stores.caches,
            },
            &mut plan.skipped,
            &mut self.resources.lockfile_verification_gate,
        )
        .await
    }
}
pub(super) fn fresh_install_context<'b>(
    install: FreshInputs<'b>,
    shape: &'b InstallShape,
    caches: &'b resolver_setup::StoreCaches,
    layout: &'b VirtualStoreLayout,
    allow_build_policy: &'b AllowBuildPolicy,
) -> pnpm_deps_restorer::InstallContext<'b> {
    pnpm_deps_restorer::InstallContext {
        config: install.config,
        workspace_root: install.lockfile_dir,
        requester: install.requester,
        layout,
        node_linker: install.node_linker,
        allow_build_policy,
        link_options: &shape.link_options,
        logged_methods: install.logged_methods,
        git_source_cache: &caches.git_source_cache,
        dir_clone_cache: None,
    }
}
