use super::{
    super::{
        PackageManifest, Path, PathBuf, UpdateSeedPolicy,
        auto_dedupe_baseline::{AutoDedupeBaseline, BaselineInputs},
    },
    InstallScope, RunExecution,
    dispatch::Settled,
};
use crate::{ProjectMutation, optimistic_repeat_install::current_settings_with_catalogs};

impl RunExecution<'_> {
    pub(super) fn wanted_lockfile_path(&self) -> PathBuf {
        self.install.context.lockfile_path.map_or_else(
            || {
                self.workspace.dirs.workspace_root.join(
                    self.install.context.config.wanted_lockfile_name(),
                )
            },
            Path::to_path_buf,
        )
    }

    /// The baseline of an `autoDedupe` `--lockfile-only` install that
    /// resolves the whole workspace against the lockfile's pins and saves
    /// what it resolves. `None` for every other install.
    pub(super) fn auto_dedupe_baseline(
        &self,
        scope: &InstallScope<'_>,
        project_manifests: &[(PathBuf, &PackageManifest)],
    ) -> Option<AutoDedupeBaseline> {
        let config = self.install.context.config;
        if !config.auto_dedupe
            || !self.mode.lockfile_only
            || self.install.lockfile_policy.frozen
            || !self.resolves_the_whole_workspace(scope)
        {
            return None;
        }
        let settings = current_settings_with_catalogs(
            config,
            self.install.execution.node_linker,
            self.mode.included,
            self.owned.projects.supported_architectures.as_ref(),
            &self.workspace.catalogs,
        );
        let inputs = BaselineInputs {
            settings: &settings,
            ignore_pnpmfile: config.ignore_pnpmfile,
            workspace_root: &self.workspace.dirs.workspace_root,
            project_manifests,
        };
        Some(AutoDedupeBaseline::new(&config.cache_dir, self.wanted_lockfile_path(), &inputs))
    }

    fn resolves_the_whole_workspace(&self, scope: &InstallScope<'_>) -> bool {
        matches!(self.install.execution.mutation, ProjectMutation::InstallWorkspace)
            && !scope.importers.filtered_install
            && matches!(self.owned.resolution.update_seed_policy, UpdateSeedPolicy::KeepAll)
            && self.owned.resolution.preferred_versions_override.is_none()
            && self.resolves_into_one_lockfile_file()
    }

    /// A dedicated lockfile's resolution also reads sibling projects the
    /// digest does not cover, and Git branch lockfiles are read from, or
    /// folded into, a file other than the one the record describes.
    fn resolves_into_one_lockfile_file(&self) -> bool {
        let config = self.install.context.config;
        config.shares_one_lockfile()
            && !config.use_git_branch_lockfile
            && !config.merge_git_branch_lockfiles
            && config.git_branch_lockfile_candidates.is_empty()
    }
}

impl Settled<'_, '_> {
    /// Whether a `--lockfile-only` install that `autoDedupe` would otherwise
    /// always resolve may trust the record and take the up-to-date path: the
    /// lockfile it loaded is the file on disk that a deduplicating resolution
    /// recorded writing under the same inputs.
    pub(super) fn trusts_dedupe_record(self) -> bool {
        let config = self.install.context.config;
        self.install.lockfile_policy.prefer_frozen.unwrap_or(config.prefer_frozen_lockfile)
            && self.recorded_lockfile_matches()
    }

    fn recorded_lockfile_matches(self) -> bool {
        let Some(baseline) = self.verification.auto_dedupe_baseline.as_ref() else {
            return false;
        };
        self.lockfiles.wanted.loaded.is_some_and(|lockfile| baseline.matches(lockfile))
    }
}
