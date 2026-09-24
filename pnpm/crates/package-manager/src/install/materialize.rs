pub(super) use scope::initial_materialization_ids;

mod frozen;
mod scope;
use scope::{
    allow_builds_changed_since, anchored_project_manifests, announce_headless_install,
    frozen_project_anchor_ids, importer_manifests_by_id, lockfile_specifier_manifests_by_id,
    previously_skipped, prior_unbuilt_builds, record_fresh_lockfile_verified,
    settle_frozen_verification,
};

use super::{
    Arc, AtomicU8, Catalogs, DependencyGroup, HashSet, HoistedDependencies, IncludedDependencies,
    InstallError, InstallFrozenLockfile, InstallWithFreshLockfile, Lockfile, LockfileEntries,
    MemCache, PackageManifest, Path, PathBuf, RebuildOptions, Reporter, ResolutionVerifier,
    ThrottledClient, build_workspace_packages_map, map_fresh_lockfile_error,
    map_frozen_lockfile_error, run::InstallView,
};
use crate::install_with_fresh_lockfile::{FreshInputs, OwnedInputs};

pub(super) struct MaterializationInputs<'a, 'install> {
    pub(super) install: InstallView<'a>,
    pub(super) resolution: MaterializationResolution<'a>,
    pub(super) lockfiles: MaterializationLockfiles<'a, 'install>,
    pub(super) workspace: MaterializationWorkspace<'a>,
    pub(super) modules: MaterializationModules<'a>,
    pub(super) execution: MaterializationExecution<'a>,
    pub(super) downloads: MaterializationDownloads,
}

pub(super) struct MaterializationWorkspace<'a> {
    pub(super) dependency_groups: Vec<DependencyGroup>,
    pub(super) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(super) lockfile_specifier_project_manifests: Option<Vec<(PathBuf, PackageManifest)>>,
    pub(super) workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    pub(super) requested_importer_ids: Option<&'a HashSet<String>>,
    pub(super) real_importer_ids: &'a HashSet<String>,
    pub(super) workspace_root: &'a Path,
    pub(super) catalogs: &'a Catalogs,
}

pub(super) struct MaterializationModules<'a> {
    pub(super) included: IncludedDependencies,
    pub(super) rebuild: Option<&'a RebuildOptions>,
    pub(super) modules_manifest: Option<&'a pnpm_modules_yaml::ModulesLayout>,
    pub(super) prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    pub(super) prior_hoisted_locations: Option<&'a pnpm_deps_restorer::HoistedLocations>,
    pub(super) prune_orphans: bool,
    /// See [`pnpm_deps_restorer::PriorMaterialization::relink_every_slot_bin`].
    pub(super) relink_every_slot_bin: bool,
    pub(super) logged_methods: &'a AtomicU8,
}

pub(super) struct MaterializationExecution<'a> {
    pub(super) effective_node_version: Option<String>,
    pub(super) take_frozen_path: bool,
    pub(super) supported_architectures:
        Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    /// A host detection spawned right after the wanted lockfile parse —
    /// see [`pnpm_deps_restorer::materialization_plan::HostDetection::spawn`].
    /// Handed to whichever install path runs.
    pub(super) early_host_detection:
        Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    pub(super) resolve_only: bool,
    pub(super) can_prompt: bool,
    pub(super) save_lockfile: bool,
    pub(super) prefix: &'a str,
}

pub(super) struct MaterializationDownloads {
    pub(super) tarball_mem_cache: Arc<MemCache>,
    pub(super) http_client_arc: Arc<ThrottledClient>,
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
    pub(super) pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) deploy_manifest_hook: bool,
    pub(super) manifest_spec_bumps: Option<&'a crate::ManifestSpecBumps>,
    pub(super) inputs: crate::install::run::ResolutionInputs,
}

/// The store-index writer's wind-down task — see
/// [`MaterializationOutput::store_index_teardown`].
pub(super) type StoreIndexTeardown =
    tokio::task::JoinHandle<Result<(), pnpm_store_dir::StoreIndexError>>;

/// What the install put on disk and what its resolution decided.
pub(super) struct Materialized {
    pub hoisted: pnpm_deps_restorer::InstalledHoistedState,
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) install_skipped: crate::SkippedSnapshots,
    /// See
    /// [`crate::InstallWithFreshLockfileResult::peer_issue_importer_ids`].
    /// Empty on the frozen path, which resolves nothing and so also
    /// leaves `fresh_lockfile` `None`.
    pub(super) peer_issue_importer_ids: HashSet<String>,
    pub(super) fresh_lockfile: Option<Lockfile>,
    /// The frozen path's peer classification of the wanted lockfile, which
    /// the current lockfile is filtered with. `None` on the fresh path.
    pub(super) groups: Option<crate::GroupSelection>,
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
    if inputs.execution.take_frozen_path {
        inputs.frozen::<Reporter>().await
    } else {
        inputs.fresh::<Reporter>().await
    }
}

/// What a frozen install materializes: the closure of the importers it
/// installs for, and the project manifests the bin links anchor on.
struct FrozenScope<'a> {
    closure: crate::MaterializationClosure,
    project_manifests: Vec<(PathBuf, &'a PackageManifest)>,
    /// The peer classification of the wanted lockfile, shared by every walk
    /// of this install.
    groups: crate::GroupSelection,
}

impl FrozenScope<'_> {
    fn lockfile(&self) -> &Lockfile {
        &self.closure.lockfile
    }
}

impl<'a> MaterializationInputs<'a, '_> {
    fn fresh_prior<'b>(
        &self,
        prior_unbuilt_builds: &'b pnpm_deps_restorer::UnbuiltBuilds,
        previously_skipped: &'b pnpm_deps_restorer::SkippedSnapshots,
    ) -> crate::install_with_fresh_lockfile::FreshPriorInstall<'b>
    where
        'a: 'b,
    {
        crate::install_with_fresh_lockfile::FreshPriorInstall {
            lockfile: self.lockfiles.current,
            hoisted_dependencies: self.modules.prior_hoisted_dependencies,
            hoisted_locations: self.modules.prior_hoisted_locations,
            unbuilt_builds: prior_unbuilt_builds,
            previously_skipped,
            allow_builds_changed: allow_builds_changed_since(
                self.modules.modules_manifest,
                self.install.context.config,
            ),
            prune_orphans: self.modules.prune_orphans,
            relink_every_slot_bin: self.modules.relink_every_slot_bin,
        }
    }

    fn fresh_inputs<'b>(
        &self,
        dependency_groups: &'b [DependencyGroup],
        resolution_verifiers: &'b [Arc<dyn ResolutionVerifier>],
        prior_unbuilt_builds: &'b pnpm_deps_restorer::UnbuiltBuilds,
        previously_skipped: &'b pnpm_deps_restorer::SkippedSnapshots,
    ) -> FreshInputs<'b>
    where
        'a: 'b,
    {
        FreshInputs {
            update_checksums: self.install.lockfile_policy.update_checksums,
            resolution_verifiers,
            drivers: crate::install_with_fresh_lockfile::FreshInstallDrivers {
                http_client: self.install.context.http_client,
                config: self.install.context.config,
                logged_methods: self.modules.logged_methods,
            },
            projects: crate::install_with_fresh_lockfile::FreshInstallProjects {
                dependency_groups,
                requester: self.execution.prefix,
                lockfile_dir: self.workspace.workspace_root,
                supported_architectures: self.execution.supported_architectures,
                is_full_install: self.install.execution.mutation.is_full_install(),
                real_ids: self.workspace.requested_importer_ids.map(|_| {
                    self.workspace.real_importer_ids
                }),
                selected_ids: self.workspace.requested_importer_ids,
            },
            execution: crate::install_with_fresh_lockfile::FreshInstallExecution {
                node_linker: self.install.execution.node_linker,
                lockfile_only: self.execution.resolve_only,
                skip_runtimes: self.install.execution.skip_runtimes,
                dry_run: self.install.execution.dry_run,
                can_prompt: self.execution.can_prompt,
                policy_excludes: self.install.lockfile_policy.excludes,
                save_lockfile: self.execution.save_lockfile,
            },
            manifests: crate::install_with_fresh_lockfile::FreshManifestOptions {
                deploy_hook: self.resolution.deploy_manifest_hook,
                spec_bumps: self.resolution.manifest_spec_bumps,
            },
            prior: self.fresh_prior(prior_unbuilt_builds, previously_skipped),
            lockfiles: crate::install_with_fresh_lockfile::FreshLockfileSeeds {
                wanted: self.lockfiles.wanted,
                merge_wanted: self.lockfiles.merge_wanted,
            },
        }
    }

    fn into_owned_inputs(
        self,
        lockfile_verification_gate: Option<crate::LockfileVerificationGate>,
    ) -> OwnedInputs {
        OwnedInputs {
            wanted_lockfile_shared: self.lockfiles.wanted_shared,
            node_version: self.execution.effective_node_version,
            early_host_detection: self.execution.early_host_detection,
            pnpmfile_hook_override: self.resolution.pnpmfile_hook,
            lockfile_verification_gate,
            resolution: crate::ResolutionInputs {
                update_seed_policy: self.resolution.inputs.update_seed_policy,
                preferred_versions_override: self.resolution.inputs.preferred_versions_override,
                auth_override: self.resolution.inputs.auth_override,
                observer: self.resolution.inputs.observer,
                peer_issues_sink: self.resolution.inputs.peer_issues_sink,
                deps_requiring_build_sink: self.resolution.inputs.deps_requiring_build_sink,
            },
            fetching: crate::install_with_fresh_lockfile::FreshFetchingInputs {
                tarball_mem_cache: self.downloads.tarball_mem_cache,
                http_client_arc: self.downloads.http_client_arc,
                meta_cache: self.lockfiles.verification.meta_cache,
            },
            projects: crate::install_with_fresh_lockfile::FreshProjectInputs {
                lockfile_specifier_manifests: self.workspace
                    .lockfile_specifier_project_manifests
                    .map(|manifests| {
                        lockfile_specifier_manifests_by_id(manifests, self.workspace.workspace_root)
                    }),
                catalogs: self.workspace.catalogs.clone(),
                workspace_packages: build_workspace_packages_map(self.workspace.workspace_projects),
            },
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
            if let Some(lockfile_verification_override) = self.lockfiles
                .verification_override
                .take()
            {
                lockfile_verification_override.await.map_err(map_frozen_lockfile_error)?;
                None
            } else {
                self.lockfiles.wanted.and_then(|loaded_lockfile| {
                    super::LockfileVerificationGate::spawn::<Reporter>(
                        loaded_lockfile,
                        &self.lockfiles.verification.resolution_verifiers,
                        self.lockfiles.verification.derived_lockfile_path.as_deref(),
                        &self.install.context.config.cache_dir,
                        self.resolution.inputs.update_seed_policy.replaced_update_targets(
                            loaded_lockfile,
                            self.workspace.requested_importer_ids,
                        ),
                    )
                })
            },
        )
    }

    async fn fresh<Reporter: self::Reporter + 'static>(
        mut self,
    ) -> Result<MaterializationOutput, InstallError> {
        let lockfile_verification_gate = self.start_fresh_verification::<Reporter>().await?;
        let dependency_groups = std::mem::take(&mut self.workspace.dependency_groups);
        let resolution_verifiers =
            std::mem::take(&mut self.lockfiles.verification.resolution_verifiers);
        let derived_lockfile_path = self.lockfiles.verification
            .derived_lockfile_path
            .take();
        let site = (self.workspace.workspace_root, self.install.context.config);
        let prior_unbuilt = prior_unbuilt_builds(self.modules.modules_manifest);
        let prior_skipped = previously_skipped(self.modules.modules_manifest);
        let fresh_result = InstallWithFreshLockfile {
            inputs: self.fresh_inputs(
                &dependency_groups,
                &resolution_verifiers,
                &prior_unbuilt,
                &prior_skipped,
            ),
            importer_manifests: importer_manifests_by_id(
                self.workspace.project_manifests,
                self.workspace.workspace_root,
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
        Ok(fresh_materialization_output(fresh_result))
    }
}

fn fresh_materialization_output(
    fresh_result: crate::install_with_fresh_lockfile::InstallWithFreshLockfileResult,
) -> MaterializationOutput {
    MaterializationOutput {
        materialized: Materialized {
            hoisted: fresh_result.hoisted,
            ignored_builds: fresh_result.ignored_builds,
            deferred_builds: fresh_result.deferred_builds,
            install_skipped: fresh_result.skipped,
            peer_issue_importer_ids: fresh_result.peer_issue_importer_ids,
            fresh_lockfile: fresh_result.wanted_lockfile,
            groups: None,
        },
        store_index_teardown: fresh_result.store_index_teardown,
    }
}

impl<'a> MaterializationWorkspace<'a> {
    fn frozen_scope(
        &self,
        lockfile: &Lockfile,
        node_linker: super::NodeLinker,
        groups: crate::GroupSelection,
        ignore_manifest_check: bool,
    ) -> FrozenScope<'a> {
        let empty_skipped = crate::SkippedSnapshots::new();
        let importer_ids = self.requested_importer_ids.or_else(|| {
            (!ignore_manifest_check).then_some(self.real_importer_ids)
        });
        let closure = crate::materialization_closure(
            lockfile,
            self.workspace_root,
            &initial_materialization_ids(lockfile, importer_ids, node_linker),
            &groups,
            &empty_skipped,
        );
        let project_anchor_ids = frozen_project_anchor_ids(
            self.requested_importer_ids,
            self.real_importer_ids,
            node_linker,
            &closure,
        );
        FrozenScope {
            project_manifests: anchored_project_manifests(
                self.project_manifests,
                self.workspace_root,
                &project_anchor_ids,
            ),
            closure,
            groups,
        }
    }
}

impl MaterializationOutput {
    fn from_frozen(
        frozen_result: pnpm_deps_restorer::InstallFrozenLockfileOutput,
        groups: crate::GroupSelection,
    ) -> Self {
        MaterializationOutput {
            materialized: Materialized {
                hoisted: frozen_result.hoisted,
                ignored_builds: frozen_result.ignored_builds,
                deferred_builds: frozen_result.deferred_builds,

                install_skipped: frozen_result.skipped,
                peer_issue_importer_ids: HashSet::new(),
                fresh_lockfile: None,
                groups: Some(groups),
            },
            store_index_teardown: frozen_result.store_index_teardown,
        }
    }
}
