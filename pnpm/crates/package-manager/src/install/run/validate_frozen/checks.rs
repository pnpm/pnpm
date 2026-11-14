use super::{
    super::{
        super::{
            Arc, CheckLockfileSettingsDriftOptions, InstallError, Lockfile, Path,
            PnpmfileChecksumCheck, VerifyLockfileResolutionsOptions, check_lockfile_settings_drift,
            parse_config_overrides, verify_lockfile_resolutions,
        },
        InstallScope, RunExecution,
        dispatch::{FrozenDispatch, decide_frozen_path},
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
        verify_lockfile_offline_and_policies(
            lockfile,
            &[],
            None,
            Some(&self.install.context.config.cache_dir),
        )
        .await?;
        self.check_pre_hook_settings_drift(lockfile).await
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
        let lockfile_path = (!is_hooked && !settled.loaded.wanted.had_conflicts)
            .then_some(settled.verification.derived_lockfile_path.as_deref())
            .flatten();
        let cache_dir = (!is_hooked).then_some(settled.install.context.config.cache_dir.as_path());
        verify_lockfile_offline_and_policies(
            lockfile,
            &settled.verification.resolution_verifiers,
            lockfile_path,
            cache_dir,
        )
        .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn verify_lockfile_offline_and_policies(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>],
    lockfile_path: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Result<(), InstallError> {
    verify_lockfile_resolutions::<SilentReporter>(
        lockfile,
        verifiers,
        &VerifyLockfileResolutionsOptions {
            concurrency: None,
            lockfile_path,
            cache_dir,
            replaced: None,
            repairs_lockfile: false,
        },
    )
    .await
    .map_err(InstallError::LockfileVerification)
}
