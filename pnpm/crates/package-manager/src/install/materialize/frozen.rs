use super::{
    FrozenScope, InstallError, InstallFrozenLockfile, Lockfile, LockfileEntries,
    MaterializationInputs, MaterializationOutput, Reporter, allow_builds_changed_since,
    announce_headless_install, map_frozen_lockfile_error, previously_skipped, prior_unbuilt_builds,
    settle_frozen_verification,
};

impl<'a> MaterializationInputs<'a, '_> {
    fn frozen_lockfiles<'b>(
        &'b self,
        scope: &'b FrozenScope<'a>,
        lockfile: &'b Lockfile,
    ) -> pnpm_deps_restorer::FrozenLockfileInputs<'b> {
        pnpm_deps_restorer::FrozenLockfileInputs {
            wanted: scope.lockfile(),
            verified: lockfile,
            path: self.lockfiles.verification.derived_lockfile_path.as_deref(),
            current: self.lockfiles.current,
            current_entries: LockfileEntries::of_previous_install(self.lockfiles.current),
            resolution_verifiers: self.workspace.requested_importer_ids.map_or(
                self.lockfiles.verification.resolution_verifiers.as_slice(),
                |_| &[][..],
            ),
            planned_canonical_fetches: Some(&self.lockfiles.verification.planned_canonical_fetches),
        }
    }

    fn take_frozen_seed<'b>(
        &mut self,
        frozen_verification_override: Option<crate::LockfileVerificationOverride<'b>>,
    ) -> pnpm_deps_restorer::FrozenInstallSeed<'b> {
        pnpm_deps_restorer::FrozenInstallSeed {
            early_host_detection: self.execution.early_host_detection.take(),
            node_version: self.execution.effective_node_version.take(),
            skipped: self.modules.modules_manifest.map(|manifest| {
                manifest.skipped.clone()
            }),
            lockfile_verification_override: frozen_verification_override,
        }
    }

    fn frozen_installer<'b>(
        &'b mut self,
        scope: &'b FrozenScope<'a>,
        lockfile: &'b Lockfile,
        frozen_verification_override: Option<crate::LockfileVerificationOverride<'b>>,
        prior_unbuilt_builds: &'b pnpm_deps_restorer::UnbuiltBuilds,
        previously_skipped: &'b pnpm_deps_restorer::SkippedSnapshots,
    ) -> InstallFrozenLockfile<'b> {
        let seed = self.take_frozen_seed(frozen_verification_override);
        InstallFrozenLockfile {
            drivers: pnpm_deps_restorer::FrozenInstallDrivers {
                config: self.install.context.config,
                http_client: self.install.context.http_client,
                pnpmfile_hook: self.resolution.pnpmfile_hook.as_ref(),
                tarball_mem_cache: Some(&self.downloads.tarball_mem_cache),
            },
            lockfiles: self.frozen_lockfiles(scope, lockfile),
            platform: pnpm_deps_restorer::FrozenPlatformOptions {
                supported_architectures: self.execution.supported_architectures,
                skip_runtimes: self.install.execution.skip_runtimes,
                node_linker: self.install.execution.node_linker,
            },
            prior: pnpm_deps_restorer::PriorMaterialization {
                rebuild: self.modules.rebuild,
                hoisted_dependencies: self.modules.prior_hoisted_dependencies,
                hoisted_locations: self.modules.prior_hoisted_locations,
                allow_builds_changed: allow_builds_changed_since(
                    self.modules.modules_manifest,
                    self.install.context.config,
                ),
                unbuilt_builds: prior_unbuilt_builds,
                previously_skipped,
                prune_orphans: self.modules.prune_orphans,
                relink_every_slot_bin: self.modules.relink_every_slot_bin,
            },
            projects: pnpm_deps_restorer::FrozenProjectInputs {
                workspace_root: self.workspace.workspace_root,
                requester: self.execution.prefix,
                dependency_groups: &self.workspace.dependency_groups,
                manifests: &scope.project_manifests,
                package_map_manifests: self.workspace.project_manifests,
            },
            seed,

            logged_methods: self.modules.logged_methods,
        }
    }

    pub(super) async fn frozen<Reporter: self::Reporter + 'static>(
        mut self,
    ) -> Result<MaterializationOutput, InstallError> {
        let lockfile = self.lockfiles.wanted.expect("dispatch verified lockfile is present");
        announce_headless_install::<Reporter>(
            lockfile,
            self.modules.rebuild,
            self.install.lockfile_policy.ignore_manifest_check
                && !self.install.execution.mutation.is_full_install(),
            self.execution.prefix,
        );
        let scope = self.workspace.frozen_scope(
            lockfile,
            self.install.execution.node_linker,
            self.modules.included,
            self.install.lockfile_policy.ignore_manifest_check,
        );
        let supported_lockfile_major = matches!(scope.lockfile().lockfile_version.major, 9 | 12);
        debug_assert!(supported_lockfile_major);

        let frozen_verification_override = settle_frozen_verification::<Reporter>(
            self.workspace.requested_importer_ids,
            self.lockfiles.verification_override.take(),
            lockfile,
            &self.lockfiles.verification.resolution_verifiers,
            self.lockfiles.verification.derived_lockfile_path.as_deref(),
            &self.install.context.config.cache_dir,
        )
        .await?;
        let prior_unbuilt = prior_unbuilt_builds(self.modules.modules_manifest);
        let prior_skipped = previously_skipped(self.modules.modules_manifest);
        let frozen_result = self
            .frozen_installer(
                &scope,
                lockfile,
                frozen_verification_override,
                &prior_unbuilt,
                &prior_skipped,
            )
            .run::<Reporter>()
            .await
            // Surface a verification failure as the same top-level
            // `LockfileVerification` variant the eager paths use, rather
            // than nesting it under `FrozenLockfile` — the concurrent gate
            // is the same gate, just run alongside the fetch.
            .map_err(map_frozen_lockfile_error)?;
        Ok(MaterializationOutput::from_frozen(frozen_result))
    }
}
