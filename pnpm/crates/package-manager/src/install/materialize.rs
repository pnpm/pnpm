mod scope;
use scope::{
    allow_builds_changed_since, anchored_project_manifests, announce_headless_install,
    frozen_project_anchor_ids, importer_manifests_by_id, initial_materialization_ids,
    lockfile_specifier_manifests_by_id, prior_unbuilt_builds, record_fresh_lockfile_verified,
    settle_frozen_verification,
};

use super::{
    Arc, AtomicU8, AuthHeaders, BTreeMap, Catalogs, DependencyGroup, DepsRequiringBuildSink,
    HashSet, HoistedDependencies, IncludedDependencies, InstallError, InstallFrozenLockfile,
    InstallWithFreshLockfile, Lockfile, LockfileEntries, MemCache, PackageManifest, Path, PathBuf,
    PeerIssuesSink, RebuildOptions, Reporter, ResolutionVerifier, ThrottledClient,
    UpdateSeedPolicy, build_workspace_packages_map, map_fresh_lockfile_error,
    map_frozen_lockfile_error, run::InstallView,
};
use crate::install_with_fresh_lockfile::{FreshInputs, OwnedInputs};

pub(super) struct MaterializationInputs<'a, 'install> {
    pub(super) install: InstallView<'a>,
    pub(super) effective_node_version: Option<String>,
    pub(super) tarball_mem_cache: Arc<MemCache>,
    pub(super) http_client_arc: Arc<ThrottledClient>,

    pub(super) take_frozen_path: bool,

    pub(super) dependency_groups: Vec<DependencyGroup>,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) lockfile_specifier_project_manifests: Option<Vec<(PathBuf, PackageManifest)>>,
    pub(super) workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) real_importer_ids: &'a HashSet<String>,
    pub(super) workspace_root: &'a Path,
    pub(super) included: IncludedDependencies,
    pub(super) rebuild: Option<&'a RebuildOptions>,

    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// A host detection spawned right after the wanted lockfile parse —
    /// see [`pnpm_deps_restorer::materialization_plan::HostDetection::spawn`].
    /// Handed to whichever install path runs.
    pub(super) early_host_detection:
        Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    pub(super) prior_hoisted_locations: Option<&'a pnpm_deps_restorer::HoistedLocations>,

    pub(super) prune_orphans: bool,
    pub(super) logged_methods: &'a AtomicU8,

    pub(super) resolve_only: bool,
    pub(super) can_prompt: bool,
    pub(super) save_lockfile: bool,
    pub(super) catalogs: &'a Catalogs,
    pub(super) resolution: MaterializationResolution<'a>,
    pub(super) prefix: &'a str,
    pub(super) lockfiles: MaterializationLockfiles<'a, 'install>,
}

pub(super) struct MaterializationLockfiles<'a, 'install> {
    pub(super) wanted: Option<&'a Lockfile>,
    /// An `Arc` handle to the same document as [`Self::wanted`], when
    /// the lazy loader holds one. Lets the fresh path seed the resolver
    /// without deep-copying a workspace-scale lockfile; `None` falls
    /// back to the copy.
    pub(super) wanted_shared: Option<Arc<Lockfile>>,
    pub(super) merge_wanted: Option<&'a Lockfile>,
    pub(super) current: Option<&'a Lockfile>,
    pub(super) verification_override: Option<super::LockfileVerificationOverride<'install>>,
    pub(super) verification: super::run::Verification,
}

pub(super) struct MaterializationResolution<'a> {
    pub(super) update_seed_policy: UpdateSeedPolicy,
    pub(super) preferred_versions_override: Option<pnpm_resolving_resolver_base::PreferredVersions>,
    pub(super) auth_override: Option<Arc<AuthHeaders>>,
    pub(super) resolution_observer: Option<Arc<dyn crate::ResolutionObserver>>,
    pub(super) peer_issues_sink: Option<PeerIssuesSink>,
    pub(super) deps_requiring_build_sink: Option<DepsRequiringBuildSink>,
    pub(super) pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) deploy_manifest_hook: bool,
    pub(super) manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
}

/// The store-index writer's wind-down task — see
/// [`MaterializationOutput::store_index_teardown`].
pub(super) type StoreIndexTeardown =
    tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>;

/// What the install put on disk and what its resolution decided.
pub(super) struct Materialized {
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) injected_deps: BTreeMap<String, Vec<String>>,
    pub(super) hoisted_dependencies: HoistedDependencies,
    pub(super) hoisted_locations: BTreeMap<String, Vec<String>>,
    pub(super) install_skipped: crate::SkippedSnapshots,
    /// See
    /// [`crate::InstallWithFreshLockfileResult::peer_issue_importer_ids`].
    /// Empty on the frozen path, which resolves nothing and so also
    /// leaves `fresh_lockfile` `None`.
    pub(super) peer_issue_importer_ids: HashSet<String>,
    pub(super) fresh_lockfile: Option<Lockfile>,
}

pub(super) struct MaterializationOutput {
    pub(super) materialized: Materialized,
    /// The store-index writer task, already winding down (both install
    /// paths dropped every writer handle before returning). The caller
    /// awaits it after the tail writes it can overlap with — the full
    /// rationale lives at the await site in `run.rs`.
    pub(super) store_index_teardown: StoreIndexTeardown,
}

pub(super) async fn materialize<Reporter: self::Reporter + 'static>(
    inputs: MaterializationInputs<'_, '_>,
) -> Result<MaterializationOutput, InstallError> {
    if inputs.take_frozen_path {
        inputs.frozen::<Reporter>().await
    } else {
        inputs.fresh::<Reporter>().await
    }
}

/// What a frozen install materializes: the closure of the requested
/// importers under the isolated linker, and the project manifests the
/// bin links anchor on.
struct FrozenScope<'a> {
    closure: Option<crate::MaterializationClosure>,
    project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
}

impl FrozenScope<'_> {
    fn lockfile<'l>(&'l self, lockfile: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(lockfile, |closure| &closure.lockfile)
    }
}

impl<'a> MaterializationInputs<'a, '_> {
    fn frozen_scope(&self, lockfile: &Lockfile) -> FrozenScope<'a> {
        let empty_skipped = crate::SkippedSnapshots::new();
        let closure = initial_materialization_ids(
            lockfile,
            self.requested_importer_ids,
            self.install.node_linker,
        )
        .as_ref()
        .map(|importer_ids| {
            crate::materialization_closure(
                lockfile,
                self.workspace_root,
                importer_ids,
                self.included,
                &empty_skipped,
            )
        });
        let project_anchor_ids = frozen_project_anchor_ids(
            self.requested_importer_ids,
            self.real_importer_ids,
            self.install.node_linker,
            closure.as_ref(),
        );
        FrozenScope {
            project_manifests: anchored_project_manifests(
                self.project_manifests,
                self.workspace_root,
                &project_anchor_ids,
            ),
            closure,
        }
    }

    fn frozen_installer<'b>(
        &'b mut self,
        scope: &'b FrozenScope<'a>,
        lockfile: &'b Lockfile,
        frozen_verification_override: Option<super::LockfileVerificationOverride<'b>>,
        prior_unbuilt_builds: &'b pnpm_deps_restorer::UnbuiltBuilds,
    ) -> InstallFrozenLockfile<'b> {
        InstallFrozenLockfile {
            http_client: self.install.http_client,
            config: self.install.config,
            pnpmfile_hook: self.resolution.pnpmfile_hook.as_ref(),
            lockfile: scope.lockfile(lockfile),
            resolution_verifiers: self
                .requested_importer_ids
                .map_or(self.lockfiles.verification.resolution_verifiers.as_slice(), |_| &[][..]),
            lockfile_verification_override: frozen_verification_override,
            lockfile_path: self.lockfiles.verification.derived_lockfile_path.as_deref(),
            current_lockfile: self.lockfiles.current,
            current_entries: LockfileEntries::of_previous_install(
                self.lockfiles.current,
                self.install.config.force,
            ),
            dependency_groups: &self.dependency_groups,
            project_manifests: &scope.project_manifests,
            package_map_project_manifests: self.project_manifests,
            logged_methods: self.logged_methods,
            workspace_root: self.workspace_root,
            requester: self.prefix,
            supported_architectures: self.supported_architectures,
            skip_runtimes: self.install.skip_runtimes,
            node_version: self.effective_node_version.take(),
            early_host_detection: self.early_host_detection.take(),
            node_linker: self.install.node_linker,
            tarball_mem_cache: Some(&self.tarball_mem_cache),
            seed_skipped: self.modules_manifest.map(|manifest| manifest.skipped.clone()),
            rebuild: self.rebuild,
            prior_hoisted_dependencies: self.prior_hoisted_dependencies,
            prior_hoisted_locations: self.prior_hoisted_locations,
            prior_unbuilt_builds,
            allow_builds_changed: allow_builds_changed_since(
                self.modules_manifest,
                self.install.config,
            ),
            prune_orphans: self.prune_orphans,
            planned_canonical_fetches: Some(&self.lockfiles.verification.planned_canonical_fetches),
        }
    }

    async fn frozen<Reporter: self::Reporter + 'static>(
        mut self,
    ) -> Result<MaterializationOutput, InstallError> {
        let lockfile = self.lockfiles.wanted.expect("dispatch verified lockfile is present");
        announce_headless_install::<Reporter>(
            lockfile,
            self.rebuild,
            self.install.ignore_manifest_check && !self.install.mutation.is_full_install(),
            self.prefix,
        );
        let scope = self.frozen_scope(lockfile);
        let supported_lockfile_major =
            matches!(scope.lockfile(lockfile).lockfile_version.major, 9 | 12);
        debug_assert!(supported_lockfile_major);

        let frozen_verification_override = settle_frozen_verification::<Reporter>(
            self.requested_importer_ids,
            self.lockfiles.verification_override.take(),
            lockfile,
            &self.lockfiles.verification.resolution_verifiers,
            self.lockfiles.verification.derived_lockfile_path.as_deref(),
            &self.install.config.cache_dir,
        )
        .await?;
        let prior_unbuilt = prior_unbuilt_builds(self.modules_manifest);
        let frozen_result = self
            .frozen_installer(&scope, lockfile, frozen_verification_override, &prior_unbuilt)
            .run::<Reporter>()
            .await
            // Surface a verification failure as the same top-level
            // `LockfileVerification` variant the eager paths use, rather
            // than nesting it under `FrozenLockfile` — the concurrent gate
            // is the same gate, just run alongside the fetch.
            .map_err(map_frozen_lockfile_error)?;
        Ok(MaterializationOutput {
            materialized: Materialized {
                ignored_builds: frozen_result.ignored_builds,
                deferred_builds: frozen_result.deferred_builds,
                injected_deps: frozen_result.injected_deps,
                hoisted_dependencies: frozen_result.hoisted_dependencies,
                hoisted_locations: frozen_result.hoisted_locations,
                install_skipped: frozen_result.skipped,
                peer_issue_importer_ids: HashSet::new(),
                fresh_lockfile: None,
            },
            store_index_teardown: frozen_result.store_index_teardown,
        })
    }

    fn fresh_inputs<'b>(
        &self,
        dependency_groups: &'b [DependencyGroup],
        resolution_verifiers: &'b [Arc<dyn ResolutionVerifier>],
        prior_unbuilt_builds: &'b pnpm_deps_restorer::UnbuiltBuilds,
    ) -> FreshInputs<'b>
    where
        'a: 'b,
    {
        FreshInputs {
            http_client: self.install.http_client,
            config: self.install.config,
            dependency_groups,
            logged_methods: self.logged_methods,
            requester: self.prefix,
            lockfile_dir: self.workspace_root,
            update_checksums: self.install.update_checksums,
            wanted_lockfile: self.lockfiles.wanted,
            merge_wanted_lockfile: self.lockfiles.merge_wanted,
            node_linker: self.install.node_linker,
            supported_architectures: self.supported_architectures,
            lockfile_only: self.resolve_only,
            skip_runtimes: self.install.skip_runtimes,
            dry_run: self.install.dry_run,
            can_prompt: self.can_prompt,
            policy_excludes: self.install.policy_excludes,
            is_full_install: self.install.mutation.is_full_install(),
            deploy_manifest_hook: self.resolution.deploy_manifest_hook,
            real_importer_ids: self.requested_importer_ids.map(|_| self.real_importer_ids),
            selected_importer_ids: self.requested_importer_ids,
            current_lockfile: self.lockfiles.current,
            prior_hoisted_dependencies: self.prior_hoisted_dependencies,
            prior_hoisted_locations: self.prior_hoisted_locations,
            prior_unbuilt_builds,
            allow_builds_changed: allow_builds_changed_since(
                self.modules_manifest,
                self.install.config,
            ),
            prune_orphans: self.prune_orphans,
            save_lockfile: self.save_lockfile,
            manifest_spec_bumps: self.resolution.manifest_spec_bumps,
            resolution_verifiers,
        }
    }

    fn into_owned_inputs(
        self,
        lockfile_verification_gate: Option<crate::LockfileVerificationGate>,
    ) -> OwnedInputs {
        OwnedInputs {
            update_seed_policy: self.resolution.update_seed_policy,
            tarball_mem_cache: self.tarball_mem_cache,
            http_client_arc: self.http_client_arc,
            lockfile_specifier_manifests: self.lockfile_specifier_project_manifests.map(
                |manifests| lockfile_specifier_manifests_by_id(manifests, self.workspace_root),
            ),
            catalogs: self.catalogs.clone(),
            workspace_packages: build_workspace_packages_map(self.workspace_projects),
            wanted_lockfile_shared: self.lockfiles.wanted_shared,
            node_version: self.effective_node_version,
            early_host_detection: self.early_host_detection,
            meta_cache: self.lockfiles.verification.meta_cache,
            preferred_versions_override: self.resolution.preferred_versions_override,
            auth_override: self.resolution.auth_override,
            resolution_observer: self.resolution.resolution_observer,
            peer_issues_sink: self.resolution.peer_issues_sink,
            deps_requiring_build_sink: self.resolution.deps_requiring_build_sink,
            pnpmfile_hook_override: self.resolution.pnpmfile_hook,
            lockfile_verification_gate,
        }
    }

    /// Re-verify the existing lockfile alongside the fresh resolve,
    /// matching the pre-resolution gate: a committed lockfile that
    /// bypassed the policy locally is caught even though the resolver
    /// re-resolves from it. The fan-out's registry round trips overlap
    /// the resolve and the materialization; the verdict still gates
    /// bin linking, dependency builds, and the lockfile save inside
    /// [`InstallWithFreshLockfile`]. No-op when there's no lockfile
    /// (state 4) or verification is disabled. The pnpr override stays
    /// a blocking gate — it is a single round trip with nothing
    /// substantial to overlap.
    async fn start_fresh_verification<Reporter: self::Reporter>(
        &mut self,
    ) -> Result<Option<crate::LockfileVerificationGate>, InstallError> {
        Ok(
            if let Some(lockfile_verification_override) =
                self.lockfiles.verification_override.take()
            {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
                None
            } else {
                self.lockfiles.wanted.and_then(|loaded_lockfile| {
                    super::LockfileVerificationGate::spawn::<Reporter>(
                        loaded_lockfile,
                        &self.lockfiles.verification.resolution_verifiers,
                        self.lockfiles.verification.derived_lockfile_path.as_deref(),
                        &self.install.config.cache_dir,
                    )
                })
            },
        )
    }

    async fn fresh<Reporter: self::Reporter + 'static>(
        mut self,
    ) -> Result<MaterializationOutput, InstallError> {
        let lockfile_verification_gate = self.start_fresh_verification::<Reporter>().await?;
        let dependency_groups = std::mem::take(&mut self.dependency_groups);
        let resolution_verifiers =
            std::mem::take(&mut self.lockfiles.verification.resolution_verifiers);
        let derived_lockfile_path = self.lockfiles.verification.derived_lockfile_path.take();
        let site = (self.workspace_root, self.install.config);
        let prior_unbuilt = prior_unbuilt_builds(self.modules_manifest);
        let fresh_result = InstallWithFreshLockfile {
            inputs: self.fresh_inputs(&dependency_groups, &resolution_verifiers, &prior_unbuilt),
            importer_manifests: importer_manifests_by_id(
                self.project_manifests,
                self.workspace_root,
            ),
            owned: self.into_owned_inputs(lockfile_verification_gate),
        }
        .run::<Reporter>()
        .await
        .map_err(map_fresh_lockfile_error)?;
        record_fresh_lockfile_verified(
            &fresh_result,
            derived_lockfile_path,
            site,
            &resolution_verifiers,
        );
        Ok(MaterializationOutput {
            materialized: Materialized {
                ignored_builds: fresh_result.ignored_builds,
                deferred_builds: fresh_result.deferred_builds,
                injected_deps: fresh_result.injected_deps,
                hoisted_dependencies: fresh_result.hoisted_dependencies,
                hoisted_locations: fresh_result.hoisted_locations,
                install_skipped: fresh_result.skipped,
                peer_issue_importer_ids: fresh_result.peer_issue_importer_ids,
                fresh_lockfile: fresh_result.wanted_lockfile,
            },
            store_index_teardown: fresh_result.store_index_teardown,
        })
    }
}
