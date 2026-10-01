use super::{InstallError, RunExecution};

impl RunExecution<'_> {
    /// Turns off both repeat-install shortcuts for a hoisted install whose
    /// pnpmfile declares custom fetchers. A fetcher may serve a mutable
    /// directory without changing the lockfile, so materialization has to
    /// inspect its files. The loaded hook is kept for the rest of the install.
    pub(super) async fn check_custom_fetcher_reuse(&mut self) -> Result<(), InstallError> {
        if self.install.execution.node_linker != pnpm_config::NodeLinker::Hoisted
            || self.install.lockfile_policy.disable_optimistic_repeat
        {
            return Ok(());
        }
        let hook = super::resolve_pnpmfile_hook(
            self.install.context.config,
            &self.workspace.dirs.workspace_root,
            self.owned.projects.pnpmfile_hook_override.take(),
        )?;
        if let Some(hook) = &hook {
            let fetchers = hook
                .get_custom_fetchers()
                .await
                .map_err(|error| {
                    super::super::map_frozen_lockfile_error(
                        pnpm_deps_restorer::InstallFrozenLockfileError::CustomFetcherHook(error),
                    )
                })?;
            self.install.lockfile_policy.disable_optimistic_repeat = !fetchers.is_empty();
        }
        self.owned.projects.pnpmfile_hook_override = hook;
        Ok(())
    }
}
