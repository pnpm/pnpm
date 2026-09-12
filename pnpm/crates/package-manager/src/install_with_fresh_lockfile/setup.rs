use super::{
    FreshInputs, ManifestSlots, OwnedInputs,
    errors::InstallWithFreshLockfileError,
    fix_lockfile_copy, hook_log_fn, include_transitive_optional_dependencies,
    is_partial_workspace_selection, manifest_transforms, resolve, resolver_setup,
    seed_policy::{UpdateSeedPolicy, update_reuse_scopes},
    start_early_materialization,
};
use crate::store_init::init_store_dir_best_effort;
use pnpm_config::{Config, NodeLinker, TrustPolicy};
use pnpm_lockfile::Lockfile;
use pnpm_network::AuthHeaders;
use pnpm_reporter::Reporter;
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// What the setup phase hands the resolve phase: the registries, the
/// pick policy, the store handles and the resolver chain, plus what the
/// inputs and the resolution observer fix for the whole install.
pub(super) struct ResolverSetup {
    pub(super) workspace_packages: Option<Arc<pnpm_resolving_resolver_base::WorkspacePackages>>,
    pub(super) observer: ObserverSettings,
    pub(super) shape: InstallShape,
    pub(super) policy: crate::resolution_policy::PickPolicy,
    pub(super) registries: resolver_setup::Registries,
    pub(super) stores: resolver_setup::StoreIndexHandles,
    pub(super) chain: resolver_setup::ResolverChain,
}
/// What the inputs fix about the install before anything resolves.
pub(super) struct InstallShape {
    pub(super) is_hoisted: bool,
    pub(super) link_options: pnpm_cmd_shim::LinkBinsOptions,
    pub(super) filtered_isolated: bool,
    pub(super) verify_filtered_repair: bool,
    pub(super) include_transitive_optional_dependencies: bool,
}
impl InstallShape {
    fn derive(install: FreshInputs<'_>, update_seed_policy: &UpdateSeedPolicy) -> Self {
        let is_hoisted = matches!(install.node_linker, NodeLinker::Hoisted);
        let partial_selection = is_partial_workspace_selection(
            install.real_importer_ids,
            install.selected_importer_ids,
        );
        Self {
            is_hoisted,
            link_options: crate::shim_link_options(install.config, install.node_linker),
            filtered_isolated: partial_selection && !is_hoisted,
            verify_filtered_repair: matches!(update_seed_policy, UpdateSeedPolicy::FixLockfile)
                && partial_selection,
            include_transitive_optional_dependencies: include_transitive_optional_dependencies(
                install.is_full_install,
                install.dependency_groups,
            ),
        }
    }
}
/// What the resolution observer fixes for the resolvers when one drives
/// the install. Overrides cannot take the fast update while an observer
/// must see every resolution.
pub(super) struct ObserverSettings {
    pub(super) package_version_guard:
        Option<Arc<dyn pnpm_resolving_resolver_base::PackageVersionGuard>>,
    minimum_release_age_exclude_override: Option<Vec<String>>,
    pub(super) can_fast_update_overrides: bool,
}
impl ObserverSettings {
    fn of(observer: Option<&Arc<dyn crate::ResolutionObserver>>) -> Self {
        Self {
            package_version_guard: observer.and_then(|observer| observer.package_version_guard()),
            minimum_release_age_exclude_override: observer
                .and_then(|observer| observer.minimum_release_age_exclude_override()),
            can_fast_update_overrides: observer.is_none(),
        }
    }
}
/// Open the store, resolve the registries and build the resolver chain.
/// Consumes the auth override, the resolution observer, the workspace
/// packages and the pnpmfile override off `owned`.
pub(super) async fn set_up_resolvers<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    owned: &mut OwnedInputs,
) -> Result<ResolverSetup, InstallWithFreshLockfileError> {
    let shape = InstallShape::derive(install, &owned.update_seed_policy);
    // The pnpr override when supplied, else the config's npmrc headers;
    // shared by every registry-touching resolver below.
    let auth_headers =
        owned.auth_override.take().unwrap_or_else(|| Arc::clone(&install.config.auth_headers));
    let resolution_observer = owned.resolution_observer.take();
    let observer = ObserverSettings::of(resolution_observer.as_ref());

    let store_dir: &'static _ = &install.config.store_dir;
    // Eagerly create `files/00..ff` under the v11 store root so per-
    // tarball CAFS writes never pay a `create_dir_all` syscall on the
    // hot path.
    // See [`init_store_dir_best_effort`] for the error-degradation
    // policy shared with `create_virtual_store.rs`. Skipped under
    // `frozenStore`: the store is read-only and complete, so no
    // directory creation is attempted under its root.
    if !install.config.frozen_store {
        init_store_dir_best_effort(store_dir).await;
    }

    let registries = resolver_setup::resolve_registries(install.config)?;

    // `resolutionMode` / `minimumReleaseAge` derivations. `time_based`
    // and `pick_lowest_direct` steer the deps-resolver's per-depth
    // version pick; `full_metadata` forces the npm resolver to fetch
    // per-version `time` fields so the time-based cutoff and the
    // no-downgrade trust check have publication dates; `published_by`
    // (+exclude) is the maturity cutoff. Shared with `pacquet add`'s
    // explicit-spec pre-resolution via [`PickPolicy`] so both pick the
    // same version.
    let policy = crate::resolution_policy::PickPolicy::from_config_with_extra_excludes(
        install.config,
        observer.minimum_release_age_exclude_override.as_deref(),
    )
    .map_err(InstallWithFreshLockfileError::MinimumReleaseAgeExclude)?;

    // `caches` records, among other things, the package-status
    // progress emitted by resolve-time prefetches: `CreateVirtualStore`
    // still emits `resolved` later, but skips duplicate `fetched` /
    // `found_in_store` statuses for keys already reported here.
    let stores = resolver_setup::open_store_index_handles(install.config, store_dir).await;

    let chain = build_fresh_resolver_chain::<Reporter>(
        install,
        owned,
        &stores,
        &registries,
        &policy,
        &shape,
        ResolverAccess { auth_headers, observer: resolution_observer },
    )
    .await?;
    Ok(ResolverSetup {
        workspace_packages: owned.workspace_packages.take().map(Arc::new),
        observer,
        shape,
        policy,
        registries,
        stores,
        chain,
    })
}
pub(super) struct ResolverAccess {
    auth_headers: Arc<AuthHeaders>,
    observer: Option<Arc<dyn crate::ResolutionObserver>>,
}
pub(super) async fn build_fresh_resolver_chain<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    owned: &mut OwnedInputs,
    stores: &resolver_setup::StoreIndexHandles,
    registries: &resolver_setup::Registries,
    policy: &crate::resolution_policy::PickPolicy,
    shape: &InstallShape,
    access: ResolverAccess,
) -> Result<resolver_setup::ResolverChain, InstallWithFreshLockfileError> {
    resolver_setup::build_resolver_chain::<Reporter>(resolver_setup::ResolverChainInputs {
        config: install.config,
        store_dir: &install.config.store_dir,
        http_client_arc: &owned.http_client_arc,
        git_source_cache: &stores.caches.git_source_cache,
        tarball_mem_cache: &owned.tarball_mem_cache,
        auth_headers: &access.auth_headers,
        meta_cache: &owned.meta_cache,
        lockfile_dir: install.lockfile_dir,
        requester: install.requester,
        supported_architectures: install.supported_architectures,
        registries: &registries.by_scope,
        needs_full_metadata_for: Arc::clone(&policy.needs_full_metadata_for),
        registries_by_prefix: &registries.named,
        full_metadata: policy.full_metadata,
        wanted_lockfile: install.wanted_lockfile,
        store_index: stores.index.as_ref(),
        store_index_writer: &stores.writer,
        verified_files_cache: &stores.caches.verified_files,
        progress_reported: &stores.caches.progress_reported,
        prefetch_downloads: prefetch_downloads(install.lockfile_only, shape.filtered_isolated),
        pnpmfile_hook_override: owned.pnpmfile_hook_override.take(),
        resolution_observer: access.observer,
    })
    .await
}
/// The pnpmfile's hooks and the loggers that report their use.
pub(super) struct PnpmfileHooks {
    pub(super) pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) read_package_log: Option<pnpm_hooks::LogFn>,
    pub(super) after_all_resolved_log: Option<pnpm_hooks::LogFn>,
}
impl PnpmfileHooks {
    async fn run_pre_resolution<Reporter: self::Reporter>(
        &self,
        config: &Config,
        lockfile_dir: &Path,
        wanted_lockfile: Option<&Lockfile>,
    ) {
        if let Some(hook) = self.pnpmfile_hook.as_ref() {
            resolve::run_pre_resolution_hook::<Reporter>(
                hook,
                config,
                lockfile_dir,
                wanted_lockfile,
            )
            .await;
        }
    }

    fn load<Reporter: self::Reporter>(
        pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
        lockfile_dir: &Path,
    ) -> Self {
        let path =
            pnpmfile_hook.as_ref().and_then(|hook| hook.source_path()).map(Path::to_path_buf);
        let log = |name: &'static str| {
            path.as_ref().map(|from| hook_log_fn::<Reporter>(lockfile_dir, from, name))
        };
        PnpmfileHooks {
            read_package_log: log("readPackage"),
            after_all_resolved_log: log("afterAllResolved"),
            pnpmfile_hook,
        }
    }
}
/// What resolution reads that is settled before the resolve pass runs.
///
/// Two views the pass reads borrow values this struct owns: the
/// manifests as the pnpmfile rewrote them, and the wanted lockfile as
/// `fix-lockfile` repaired it.
pub(super) struct ResolutionPrep<Reporter> {
    pub(super) early_materializer:
        Option<Arc<crate::early_materializer::EarlyMaterializer<Reporter>>>,
    pub(super) trust: TrustGate,
    pub(super) transforms: manifest_transforms::ManifestTransforms,
    pub(super) fixed_wanted_lockfile: Option<Lockfile>,
    pub(super) wanted_lockfile_shared: Option<Arc<Lockfile>>,
    pub(super) patches: Patches,
    pub(super) hooks: PnpmfileHooks,
    pub(super) reuse: UpdateReuseScopes,
}
/// The `trustPolicy='no-downgrade'` gate, threaded into every resolve so
/// the npm resolver re-applies it to freshly picked versions. Full
/// metadata is forced on under the policy, so the picker hands the
/// resolver the per-version `time` and trust evidence the check reads.
pub(super) struct TrustGate {
    pub(super) policy: Option<TrustPolicy>,
    pub(super) exclude: Option<pnpm_config::version_policy::PackageVersionPolicy>,
}
impl TrustGate {
    fn of(config: &Config) -> Result<Self, InstallWithFreshLockfileError> {
        let patterns = config
            .trust_policy_exclude
            .as_deref()
            .filter(|patterns| !patterns.is_empty())
            .map(pnpm_config::version_policy::create_package_version_policy);
        let exclude =
            patterns.transpose().map_err(InstallWithFreshLockfileError::TrustPolicyExclude)?;
        Ok(Self { policy: resolver_trust_policy(config.trust_policy), exclude })
    }
}
/// `pnpm-workspace.yaml`'s `patchedDependencies`, resolved once per
/// install: the record grouped by package name that the resolver
/// consults at every per-node lookup to attach `(patch_hash=<hash>)` to
/// the matched package's `pkgIdWithPatchHash`, and the user's verbatim
/// keys mapped to their patch-file hashes for the lockfile's top-level
/// `patchedDependencies` block.
pub(super) struct Patches {
    pub(super) record: Option<Arc<pnpm_patching::PatchGroupRecord>>,
    pub(super) hashes: Option<BTreeMap<String, String>>,
}
impl Patches {
    fn resolve(config: &Config) -> Result<Self, InstallWithFreshLockfileError> {
        Ok(Self {
            record: config
                .resolved_patched_dependencies()
                .map_err(InstallWithFreshLockfileError::ResolvePatchedDependencies)?
                .map(Arc::new),
            hashes: config
                .patched_dependency_hashes()
                .map_err(InstallWithFreshLockfileError::CalcPatchHashes)?,
        })
    }
}
/// Where lockfile reuse is suppressed: `pacquet update` must re-resolve
/// its targets (and their subtrees) to highest-in-range, and a custom
/// resolver may widen the scope to everything via
/// `shouldRefreshResolution`.
pub(super) struct UpdateReuseScopes {
    pub(super) scope: pnpm_resolving_deps_resolver::UpdateReuseScope,
    pub(super) by_importer: BTreeMap<String, pnpm_resolving_deps_resolver::UpdateReuseScope>,
}
impl UpdateReuseScopes {
    /// A throwing `shouldRefreshResolution` hook propagates and aborts.
    async fn settle(
        update_seed_policy: &UpdateSeedPolicy,
        custom_resolvers: &[Arc<dyn pnpm_hooks::CustomResolver>],
        wanted_lockfile: Option<&Lockfile>,
    ) -> Result<Self, InstallWithFreshLockfileError> {
        let (mut scope, mut by_importer) = update_reuse_scopes(update_seed_policy);
        if custom_resolver_forces_resolve(custom_resolvers, wanted_lockfile).await? {
            scope = pnpm_resolving_deps_resolver::UpdateReuseScope::None;
            by_importer.clear();
        }
        Ok(Self { scope, by_importer })
    }
}
/// Runs between the resolvers being built and the resolve pass, in the
/// order pnpm's install applies these: the pnpmfile's pre-resolution hook
/// fires once the lockfile to resolve against is fixed, and a custom
/// resolver may still drop every reuse scope after that.
pub(super) async fn prepare_resolution<'a, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    owned: &mut OwnedInputs,
    setup: &mut ResolverSetup,
    manifests: &mut ManifestSlots<'a>,
) -> Result<ResolutionPrep<Reporter>, InstallWithFreshLockfileError> {
    let early_materializer = start_early_materialization::<Reporter>(install, owned, setup);
    let trust = TrustGate::of(install.config)?;
    let transforms = manifests.transform(
        install.config,
        &owned.catalogs,
        install.lockfile_dir,
        install.deploy_manifest_hook,
    )?;

    let fixed_wanted_lockfile =
        fix_lockfile_copy(&owned.update_seed_policy, install.wanted_lockfile);
    let wanted_lockfile = fixed_wanted_lockfile.as_ref().or(install.wanted_lockfile);
    // The repair copy above replaced the document, so the loader's
    // handle no longer describes `wanted_lockfile`.
    let wanted_lockfile_shared =
        fixed_wanted_lockfile.is_none().then_some(owned.wanted_lockfile_shared.take()).flatten();
    let patches = Patches::resolve(install.config)?;

    // Kept past the resolver hand-off (which consumes `pnpmfile_hook`) so
    // the `afterAllResolved` hook can transform the lockfile before it is
    // written.
    let hooks =
        PnpmfileHooks::load::<Reporter>(setup.chain.pnpmfile_hook.take(), install.lockfile_dir);
    hooks
        .run_pre_resolution::<Reporter>(install.config, install.lockfile_dir, wanted_lockfile)
        .await;
    let reuse = UpdateReuseScopes::settle(
        &owned.update_seed_policy,
        &setup.chain.custom_resolvers,
        wanted_lockfile,
    )
    .await?;
    Ok(ResolutionPrep {
        early_materializer,
        trust,
        transforms,
        fixed_wanted_lockfile,
        wanted_lockfile_shared,
        patches,
        hooks,
        reuse,
    })
}
/// Whether a custom resolver's `shouldRefreshResolution` hook demands a full
/// re-resolve. A throwing hook propagates and aborts.
pub(super) async fn custom_resolver_forces_resolve(
    custom_resolvers_raw: &[Arc<dyn pnpm_hooks::CustomResolver>],
    wanted_lockfile: Option<&Lockfile>,
) -> Result<bool, InstallWithFreshLockfileError> {
    let Some(lockfile) = wanted_lockfile else { return Ok(false) };
    crate::check_custom_resolver_force_resolve::check_custom_resolver_force_resolve(
        custom_resolvers_raw,
        lockfile,
    )
    .await
    .map_err(InstallWithFreshLockfileError::CustomResolverForceResolve)
}
/// The hash of the project's `.pnpmfile.{cjs,mjs}` when it exports hooks,
/// `None` otherwise. Resolution has already spawned the pnpmfile worker
/// (every `readPackage` runs through it), so the gate query is cheap.
pub(super) async fn pnpmfile_checksum(
    after_all_resolved_hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> Option<String> {
    let hook = after_all_resolved_hook?;
    hook.calculate_pnpmfile_checksum().await
}
/// A lockfile-only resolve never fetches, and a filtered isolated install
/// materializes a subset the prefetcher cannot predict.
pub(super) fn prefetch_downloads(lockfile_only: bool, filtered_isolated: bool) -> bool {
    !lockfile_only && !filtered_isolated
}
/// The trust policy the resolver enforces: `Off` means it enforces none.
pub(super) fn resolver_trust_policy(configured: TrustPolicy) -> Option<TrustPolicy> {
    match configured {
        TrustPolicy::Off => None,
        TrustPolicy::NoDowngrade => Some(TrustPolicy::NoDowngrade),
    }
}
pub(super) fn resolver_update_behavior(
    update_seed_policy: &UpdateSeedPolicy,
) -> pnpm_resolving_resolver_base::UpdateBehavior {
    if matches!(update_seed_policy, UpdateSeedPolicy::RefreshRevisions) {
        return pnpm_resolving_resolver_base::UpdateBehavior::Patches;
    }
    pnpm_resolving_resolver_base::UpdateBehavior::Off
}
