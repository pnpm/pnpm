use super::{
    InstallArgs, NodeLinkerArg, UpToDateFastPathCheck, install_already_up_to_date,
    read_root_manifest, warn_deprecated_override_version_references,
    warn_ignored_pnpm_manifest_fields,
};
use crate::cli_args::yarn_workspaces_field::{
    converts_yarn_workspaces, warn_about_workspaces_field,
};

fn report_up_to_date_install(
    root_manifest: Option<&serde_json::Value>,
    config: &pnpm_config::Config,
    up_to_date: &pnpm_package_manager::UpToDateWorkspace,
    emit: fn(&pnpm_reporter::LogEvent),
) {
    warn_ignored_pnpm_manifest_fields(root_manifest);
    warn_about_workspaces_field(config, root_manifest);
    warn_deprecated_override_version_references(config, emit);
    // The scope covers the same projects the full install path would
    // report; an up-to-date run says so too rather than going quiet
    // about what it just decided was current.
    emit(&pnpm_reporter::LogEvent::Scope(pnpm_reporter::ScopeLog {
        level: pnpm_reporter::LogLevel::Debug,
        selected: up_to_date.project_count.unwrap_or(1),
        total: up_to_date.project_count,
        workspace_prefix: config.workspace_dir
            .as_deref()
            .map(|dir| dir.to_string_lossy().into_owned()),
    }));
    let prefix = up_to_date.root.to_string_lossy().into_owned();
    emit(&pnpm_reporter::LogEvent::Pnpm(pnpm_reporter::PnpmLog {
        level: pnpm_reporter::LogLevel::Info,
        message: "Already up to date".to_string(),
        prefix: prefix.clone(),
    }));
    emit(&pnpm_reporter::LogEvent::Summary(pnpm_reporter::SummaryLog {
        level: pnpm_reporter::LogLevel::Debug,
        prefix,
    }));
}

impl InstallArgs {
    /// Run the repeat-install fast path before any of the async install
    /// machinery exists: when every gate below holds and
    /// [`install_already_up_to_date`] confirms nothing changed since the
    /// previous install, emit the same "Already up to date" + summary
    /// events [`Install::run`](pnpm_package_manager::Install::run) would and report the install as finished.
    ///
    /// The gates mirror the dispatch in [`crate::cli_args::CliArgs::run`]
    /// (which checks `recursive` / `filter` before reaching here) plus
    /// every input that would make [`Install::run`](pnpm_package_manager::Install::run) skip its own
    /// short-circuit or do extra pre-install work: an explicit
    /// `--frozen-lockfile` / `--lockfile-only`, config
    /// dependencies, Cargo or Python dependency management, and pnpmfile
    /// `updateConfig` hooks (these can mutate state the npm-only check does
    /// not cover). A configured
    /// pnpr server deliberately does NOT bail: the check decides
    /// purely locally that nothing changed, and asking the server
    /// cannot change that answer
    /// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
    ///
    /// `false` means "not decided" — the caller proceeds with the full
    /// install path, which re-runs the same check cheaply and
    /// reproduces any error with its established shape.
    pub fn finished_via_up_to_date_fast_path(
        &self,
        dir: &std::path::Path,
        config: &pnpm_config::Config,
        emit: fn(&pnpm_reporter::LogEvent),
    ) -> bool {
        if !self.fast_path_is_eligible(config) {
            return false;
        }
        let config_root = config.root_project_manifest_dir(dir).to_path_buf();
        if !pnpm_hooks::finder::find_pnpmfiles(
            &config_root,
            pnpm_package_manager::pnpmfile_selection(config),
        )
        .is_empty()
        {
            return false;
        }
        let root_manifest = read_root_manifest(&config_root, config.preferred_manifest_format);
        if converts_yarn_workspaces(config, dir, root_manifest.as_ref()) {
            return false;
        }
        let manifest_path =
            pnpm_workspace::project_manifest_path(dir, config.preferred_manifest_format);
        if !manifest_path.is_file() {
            return false;
        }
        let Ok(manifest) = pnpm_package_manifest::PackageManifest::from_path(manifest_path) else {
            return false;
        };
        let node_linker =
            self.materialization.node_linker.map_or(config.node_linker, NodeLinkerArg::into_config);
        let Some(up_to_date) = install_already_up_to_date(&UpToDateFastPathCheck {
            config,
            manifest: &manifest,
            dependency_groups: self.dependency_options.dependency_groups(config.optional).collect(),
            node_linker,
            supported_architectures: self.supported_architectures.apply_to(
                config.supported_architectures.clone(),
            ),
        }) else {
            return false;
        };
        report_up_to_date_install(root_manifest.as_ref(), config, &up_to_date, emit);
        true
    }

    /// Whether the up-to-date fast path may run at all, before it looks
    /// at the project on disk. Every flag here either asks for work the
    /// fast path cannot do, or describes an install whose verdict a
    /// single-directory probe cannot reach.
    pub(super) fn fast_path_is_eligible(&self, config: &pnpm_config::Config) -> bool {
        if self.effective_frozen_lockfile(config)
            || self.lockfile.only
            || self.lockfile.fix
            || self.materialization.force
            || self.materialization.verify_deps_before_run_install
            || !self.materialization.allow_build.is_empty()
        {
            return false;
        }
        if config.cargo.enabled || config.python.enabled || config.catalog_prune {
            return false;
        }
        // The merge flags reach `config` only in the dispatch, after this
        // check; and merging is work no up-to-date verdict can skip.
        if self.lockfile_updates.merge_git_branch_lockfiles
            || !self.lockfile_updates.merge_git_branch_lockfiles_branch_pattern.is_empty()
        {
            return false;
        }
        // Dedicated per-project lockfiles run one install per workspace
        // project; a single-dir up-to-date probe can't speak for the
        // sibling projects, so the loop (whose per-project engine runs
        // each have their own optimistic short-circuit) must always run.
        if !config.shares_one_lockfile() && config.workspace_dir.is_some() {
            return false;
        }
        config.config_dependencies.as_ref().is_none_or(std::collections::BTreeMap::is_empty)
    }
}
