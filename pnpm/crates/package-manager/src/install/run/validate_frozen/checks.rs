use super::{
    super::{
        super::{
            Arc,
            CheckLockfileSettingsDriftOptions,
            ExoticSubdepsPolicy,
            InstallError,
            Lockfile,
            Path,
            PnpmfileChecksumCheck,
            VerifyLockfileResolutionsOptions,
            check_lockfile_settings_drift,
            parse_config_overrides,
            verify_lockfile_resolutions,
        },
        InstallScope,
        RunExecution,
        dispatch::{
            FrozenDispatch,
            decide_frozen_path,
        },
        frozen_local_tarballs::local_tarballs_keep_frozen_path,
        manifests::resolve_pnpmfile_hook,
        settled::Settled,
    },
    SilentReporter,
};

impl RunExecution<'_> {
    /// Reject missing, structurally invalid, or settings-drifted lockfiles
    /// before spawning `.pnpmfile.cjs` hooks.
    pub(super) async fn validate_raw_wanted_before_hooks(
        &mut self,
        scope: &InstallScope<'_>,
        lockfile: Option<&Lockfile>,
    ) -> Result<(), InstallError> {
        let Some(lockfile) = lockfile else {
            if scope.project_manifests
                .iter()
                .any(|(_, manifest)| crate::install_state::manifest_has_runtime_deps(manifest))
            {
                return Err(InstallError::NoLockfile);
            }
            return Ok(());
        };
        let (base_policy, has_pnpmfile_hook) = self.build_pre_hook_local_policy(scope);
        self.verify_pre_hook_and_drift(lockfile, base_policy, has_pnpmfile_hook).await
    }

    fn build_pre_hook_local_policy(
        &self,
        scope: &InstallScope<'_>,
    ) -> (pnpm_lockfile_verification::LocalResolutionPolicy, bool) {
        let config = self.install.context.config;
        let has_pnpmfile_hook = !config.ignore_pnpmfile
            && (self.owned.projects.pnpmfile_hook_override.is_some()
                || !pnpm_hooks::finder::find_pnpmfiles(
                    config.lockfile_dir_for(&self.workspace.dirs.workspace_root),
                    crate::pnpmfile_selection(config),
                )
                .is_empty());
        let overrides_root = self.options.manifests.overrides_root_dir;
        let containment_root =
            overrides_root.unwrap_or(&self.workspace.dirs.workspace_manifest_dir);
        let with_live = pnpm_deps_restorer::attach_live_project_manifests(
            pnpm_deps_restorer::local_resolution_policy_for_workspace_with_catalogs(
                config,
                Some(&self.workspace.catalogs),
                (
                    overrides_root,
                    containment_root,
                    config.lockfile_dir_for(&self.workspace.dirs.workspace_root),
                ),
                has_pnpmfile_hook,
            )
            .with_deploy(matches!(self.install.execution.mutation, crate::ProjectMutation::Deploy)),
            scope.project_manifests
                .iter()
                .chain(scope.workspace_projects.iter().flatten()),
        );
        let policy = if self.install.lockfile_policy.ignore_manifest_check {
            with_live
        } else {
            pnpm_deps_restorer::attach_selected_project_manifests(
                with_live,
                &scope.project_manifests,
            )
        };
        (policy, has_pnpmfile_hook)
    }

    async fn verify_pre_hook_and_drift(
        &mut self,
        lockfile: &Lockfile,
        base_policy: pnpm_lockfile_verification::LocalResolutionPolicy,
        has_pnpmfile_hook: bool,
    ) -> Result<(), InstallError> {
        let cache_dir = Some(self.install.context.config.cache_dir.as_path());
        match verify_lockfile_offline_and_policies(
            lockfile,
            &[],
            (ExoticSubdepsPolicy::Allow, Some(base_policy.clone())),
            (None, cache_dir),
        )
        .await
        {
            Ok(()) => self.check_pre_hook_settings_drift(lockfile).await,
            Err(InstallError::LockfileVerification(
                pnpm_lockfile_verification::VerifyError::LocalSubdepNotAllowed { .. },
            )) if has_pnpmfile_hook || base_policy.has_child_ranged_edges() => {
                self.check_pre_hook_settings_drift(lockfile).await?;
                let enriched = pnpm_deps_restorer::enrich_local_policy_for_install(
                    base_policy,
                    lockfile,
                    pnpm_deps_restorer::LocalPolicyEnrichDrivers {
                        config: Some(self.install.context.config),
                        http_client: Some(self.install.context.http_client),
                        hook: self.owned.projects.pnpmfile_hook_override.as_deref(),
                    },
                )
                .await;
                verify_lockfile_offline_and_policies(
                    lockfile,
                    &[],
                    (ExoticSubdepsPolicy::Allow, Some(enriched)),
                    (None, cache_dir),
                )
                .await
            }
            Err(err) => Err(err),
        }
    }

    async fn check_pre_hook_settings_drift(
        &mut self,
        lockfile: &Lockfile,
    ) -> Result<(), InstallError> {
        let pnpmfile_hook = resolve_pnpmfile_hook(
            self.install.context.config,
            &self.workspace.dirs.workspace_root,
            self.owned.projects.pnpmfile_hook_override.take(),
        )?;
        if self.install.lockfile_policy.ignore_settings_check {
            self.owned.projects.pnpmfile_hook_override = pnpmfile_hook;
            return Ok(());
        }
        let pre_hook_checksum = pnpm_hooks::current_pnpmfile_checksum(
            pnpmfile_hook.as_ref(),
            lockfile.pnpmfile_checksum.as_deref(),
        )
        .await;
        let skip_checksum = self.install.context.config.ignore_pnpmfile && pnpmfile_hook.is_none();
        self.owned.projects.pnpmfile_hook_override = pnpmfile_hook;
        let parsed_overrides =
            parse_config_overrides(self.install.context.config, &self.workspace.catalogs)?;
        check_lockfile_settings_drift(
            lockfile,
            self.install.context.config,
            &self.workspace.catalogs,
            CheckLockfileSettingsDriftOptions {
                parsed_overrides: parsed_overrides.as_deref(),
                pnpmfile_checksum: if skip_checksum {
                    PnpmfileChecksumCheck::Skip
                } else {
                    PnpmfileChecksumCheck::Current(pre_hook_checksum.as_deref())
                },
            },
        )?;
        Ok(())
    }
}

pub(super) async fn verify_settled_frozen_path(
    settled: Settled<'_, '_>,
) -> Result<bool, InstallError> {
    let take_frozen_path =
        decide_frozen_path::<SilentReporter>(&FrozenDispatch::new(settled, None)).await?;
    if take_frozen_path && !settled.mode.lockfile_only {
        local_tarballs_keep_frozen_path(settled).await?;
    }
    if let Some(lockfile) = settled.lockfiles.wanted.get() {
        let is_hooked = settled.lockfiles.wanted.is_hooked();
        let config = settled.install.context.config;
        let lockfile_source = settled.verification.cache_source();
        let cache_dir = (!is_hooked).then_some(config.cache_dir.as_path());
        verify_lockfile_offline_and_policies(
            lockfile,
            &settled.verification.resolution_verifiers,
            (
                settled.verification.exotic_subdeps.clone(),
                Some(settled.verification.local_policy.clone()),
            ),
            (lockfile_source, cache_dir),
        )
        .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn verify_lockfile_offline_and_policies(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    (exotic_subdeps, local_policy): (
        ExoticSubdepsPolicy,
        Option<pnpm_lockfile_verification::LocalResolutionPolicy>,
    ),
    (lockfile_source, cache_dir): (
        Option<pnpm_lockfile_verification::LockfileCacheSource<'_>>,
        Option<&Path>,
    ),
) -> Result<(), InstallError> {
    verify_lockfile_resolutions::<SilentReporter>(
        lockfile,
        verifiers,
        &VerifyLockfileResolutionsOptions {
            concurrency: None,
            lockfile_path: lockfile_source.map(|source| source.path),
            lockfile_stat: lockfile_source.and_then(|source| source.stat.cloned()),
            cache_dir,
            replaced: None,
            repairs_lockfile: false,
            exotic_subdeps,
            local_policy,
        },
    )
    .await
    .map_err(InstallError::LockfileVerification)
}
