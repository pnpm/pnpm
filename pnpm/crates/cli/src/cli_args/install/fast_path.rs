use super::{
    InstallArgs, NodeLinkerArg, UpToDateFastPathCheck, install_already_up_to_date,
    install_already_up_to_date_from_records, install_already_up_to_date_per_project,
    read_root_manifest_json, warn_deprecated_override_version_references,
    warn_ignored_pnpm_manifest_fields,
};
use crate::cli_args::yarn_workspaces_field::{
    converts_yarn_workspaces, warn_about_workspaces_field,
};

/// Which up-to-date attempt runs.
#[derive(Clone, Copy)]
enum Attempt {
    /// Before the config setup. It decides from the records or the files,
    /// and emits the config warnings the install would, which nothing has
    /// emitted yet.
    BeforeConfig,
    /// After the config setup, which emitted the config warnings already. It
    /// decides from the records alone.
    AfterConfig,
}

/// The config warnings the install emits before its own output.
fn warn_about_config(
    root_manifest: Option<&serde_json::Value>,
    config: &pnpm_config::Config,
    emit: fn(&pnpm_reporter::LogEvent),
) {
    warn_ignored_pnpm_manifest_fields(root_manifest);
    warn_about_workspaces_field(config, root_manifest);
    warn_deprecated_override_version_references(config, emit);
}

/// Check what the projects `up_to_date` covers require of the host, as the
/// install does. The root project is checked here only before the config
/// setup, which checks it otherwise. Returns the warnings about the other
/// projects, so that a declined fast path emits none of them.
fn check_host_requirements(
    manifest_path: &std::path::Path,
    config: &pnpm_config::Config,
    up_to_date: &pnpm_package_manager::UpToDateWorkspace,
    attempt: Attempt,
    emit: fn(&pnpm_reporter::LogEvent),
) -> Result<Vec<pnpm_reporter::LogEvent>, &'static str> {
    let root_dir = crate::state::root_project_dir(manifest_path, config);
    let project_warnings = crate::state::ProjectHost::new(config, root_dir)
        .check_projects(&up_to_date.host_requirements)
        .map_err(|_| "a workspace project's host requirements are not met")?;
    if matches!(attempt, Attempt::BeforeConfig) {
        crate::state::check_root_project_engine(manifest_path, config, emit)
            .map_err(|_| "root project engine check failed")?;
    }
    Ok(project_warnings)
}

fn report_up_to_date_install(
    config: &pnpm_config::Config,
    up_to_date: &pnpm_package_manager::UpToDateWorkspace,
    emit: fn(&pnpm_reporter::LogEvent),
) {
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
    /// A configured
    /// pnpr server deliberately does NOT bail: the check decides
    /// purely locally that nothing changed, and asking the server
    /// cannot change that answer
    /// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
    ///
    /// `false` means "not decided" — the caller proceeds with the full
    /// install path, which re-runs the same check cheaply and
    /// reproduces any error with its established shape. The
    /// `pacquet::install` trace says why the path was not taken.
    pub fn finished_via_up_to_date_fast_path(
        &self,
        dir: &std::path::Path,
        config: &pnpm_config::Config,
        emit: fn(&pnpm_reporter::LogEvent),
    ) -> bool {
        match self.up_to_date_fast_path(dir, config, emit) {
            Ok(()) => {
                tracing::debug!(
                    target: "pacquet::install",
                    "repeat-install fast path taken before the install setup; nothing changed since the previous install",
                );
                true
            }
            Err(reason) => {
                tracing::debug!(
                    target: "pacquet::install",
                    reason,
                    "repeat-install fast path not taken before the install setup; the install decides",
                );
                false
            }
        }
    }

    fn up_to_date_fast_path(
        &self,
        dir: &std::path::Path,
        config: &pnpm_config::Config,
        emit: fn(&pnpm_reporter::LogEvent),
    ) -> Result<(), &'static str> {
        if let Some(reason) = self.fast_path_ineligibility(config) {
            return Err(reason);
        }
        check_config_fast_path(config, config.root_project_manifest_dir(dir))?;
        self.report_if_up_to_date(dir, config, emit, Attempt::BeforeConfig)
    }

    /// The up-to-date fast path once the config dependencies are installed
    /// and the `updateConfig` hooks have shaped `config`, for an install the
    /// pre-runtime one declined only while a pnpmfile or a config dependency
    /// could still change the configuration. It decides from the manifest
    /// records alone, so a declined attempt leaves reading the projects to
    /// the install. `false` when it is not taken.
    pub(crate) fn finished_from_records_after_config(
        &self,
        dir: &std::path::Path,
        config: &pnpm_config::Config,
        emit: fn(&pnpm_reporter::LogEvent),
    ) -> bool {
        let config_root = config.root_project_manifest_dir(dir);
        // A filter, from the command line or an `updateConfig` hook, leaves
        // the selection and its `failIfNoMatch` error to the install, as the
        // pre-runtime attempt does.
        if check_config_fast_path(config, config_root).is_ok()
            || self.fast_path_ineligibility(config).is_some()
            || !config.filter.is_empty()
            || !config.filter_prod.is_empty()
            || self.ci_may_freeze_the_lockfile(config)
        {
            return false;
        }
        // The install turns its shortcut off for a hoisted install whose
        // pnpmfile declares custom fetchers, which may serve a directory that
        // changed with the lockfile unchanged. Asking means loading the hooks,
        // which is the install's job.
        let node_linker =
            self.materialization.node_linker.map_or(config.node_linker, NodeLinkerArg::into_config);
        if node_linker == pnpm_config::NodeLinker::Hoisted && uses_pnpmfile(config, config_root) {
            return false;
        }
        let decided = self.report_if_up_to_date(dir, config, emit, Attempt::AfterConfig);
        if let Err(reason) = decided {
            tracing::debug!(
                target: "pacquet::install",
                reason,
                "repeat-install fast path not taken after the config setup; the install decides",
            );
            return false;
        }
        tracing::debug!(
            target: "pacquet::install",
            "repeat-install fast path taken after the config setup; nothing changed since the previous install",
        );
        true
    }

    /// Whether CI's default may freeze the lockfile, which the install
    /// decides once the lockfile is loaded. A frozen install never
    /// short-circuits.
    fn ci_may_freeze_the_lockfile(&self, config: &pnpm_config::Config) -> bool {
        self.configured_frozen_lockfile(config).is_none()
            && config.ci
            && !self.lockfile.only
            && self.prefer_frozen_override().unwrap_or(config.prefer_frozen_lockfile)
    }

    /// Report the install up to date when `attempt` finds it so.
    fn report_if_up_to_date(
        &self,
        dir: &std::path::Path,
        config: &pnpm_config::Config,
        emit: fn(&pnpm_reporter::LogEvent),
        attempt: Attempt,
    ) -> Result<(), &'static str> {
        let config_root = config.root_project_manifest_dir(dir);
        let root_manifest = read_root_manifest_json(config_root);
        if converts_yarn_workspaces(config, dir, root_manifest.as_ref()) {
            return Err("the manifest's workspaces field is converted");
        }
        let manifest_path = dir.join("package.json");
        if !manifest_path.is_file() {
            return Err("there is no package.json");
        }
        let manifest = pnpm_package_manifest::PackageManifest::from_path(manifest_path.clone())
            .map_err(|_| "package.json cannot be parsed")?;
        let node_linker =
            self.materialization.node_linker.map_or(config.node_linker, NodeLinkerArg::into_config);
        // Each project keeps its own lockfile and state, so each is decided
        // from its own record.
        let per_project = !config.shares_one_lockfile() && config.workspace_dir.is_some();
        let decide = match attempt {
            _ if per_project => install_already_up_to_date_per_project,
            Attempt::BeforeConfig => install_already_up_to_date,
            Attempt::AfterConfig => install_already_up_to_date_from_records,
        };
        let up_to_date = decide(&UpToDateFastPathCheck {
            config,
            manifest: &manifest,
            dependency_groups: self.dependency_options.dependency_groups(config.optional).collect(),
            node_linker,
            supported_architectures: self.supported_architectures.apply_to(
                config.supported_architectures.clone(),
            ),
        })?;
        let project_warnings =
            check_host_requirements(&manifest_path, config, &up_to_date, attempt, emit)?;
        if matches!(attempt, Attempt::BeforeConfig) {
            warn_about_config(root_manifest.as_ref(), config, emit);
        }
        project_warnings.iter().for_each(emit);
        report_up_to_date_install(config, &up_to_date, emit);
        Ok(())
    }

    /// Why the up-to-date fast path may not run at all, before it looks
    /// at the project on disk; `None` when it may. Every flag here either
    /// asks for work the fast path cannot do, or describes an install
    /// whose verdict a single-directory probe cannot reach.
    pub(super) fn fast_path_ineligibility(
        &self,
        config: &pnpm_config::Config,
    ) -> Option<&'static str> {
        let gates = [
            (self.effective_frozen_lockfile(config), "the lockfile is frozen"),
            (self.lockfile.only, "--lockfile-only"),
            (self.lockfile.fix, "--fix-lockfile"),
            (self.materialization.force, "--force"),
            (
                self.materialization.verify_deps_before_run_install,
                "verifyDepsBeforeRun asked for the install",
            ),
            (!self.materialization.allow_build.is_empty(), "--allow-build"),
            // The fast path registers the project in the store, which a dry
            // run must not write.
            (self.materialization.dry_run, "--dry-run"),
            // The flag reaches only the install's lockfile options, so the
            // settings the check compares still hold the configured value.
            (
                super::resolve_bool_override(
                    self.lockfile_updates.trust_lockfile,
                    self.lockfile_updates.no_trust_lockfile,
                    config.trust_lockfile,
                ) != config.trust_lockfile,
                "a flag overrides trustLockfile",
            ),
            (config.cargo.enabled || config.python.enabled, "another ecosystem installs too"),
            (config.catalog_prune, "catalogPrune is on"),
            // The merge flags reach `config` only in the dispatch, after this
            // check; and merging is work no up-to-date verdict can skip.
            (
                self.lockfile_updates.merge_git_branch_lockfiles
                    || !self.lockfile_updates
                        .merge_git_branch_lockfiles_branch_pattern
                        .is_empty(),
                "git branch lockfiles are merged",
            ),
        ];
        gates
            .into_iter()
            .find_map(|(blocked, reason)| blocked.then_some(reason))
    }
}

/// The gates the config setup resolves: config dependencies to install or
/// reconcile, and pnpmfiles whose `updateConfig` hooks may change `config`.
fn check_config_fast_path(
    config: &pnpm_config::Config,
    config_root: &std::path::Path,
) -> Result<(), &'static str> {
    if config.config_dependencies
        .as_ref()
        .is_some_and(|deps| !deps.is_empty())
    {
        return Err("config dependencies are declared");
    }
    if pnpm_env_installer::read_config_dependency_lockfile(
        config.config_dependencies.as_ref(),
        config_root,
        pnpm_env_installer::ConfigDepsInstallMode::Reconcile,
    )
    .map_err(|_| "config dependency state cannot be read")?
    .is_some()
    {
        return Err("config dependencies need reconciling");
    }
    if uses_pnpmfile(config, config_root) {
        return Err("a pnpmfile is in use");
    }
    Ok(())
}

fn uses_pnpmfile(config: &pnpm_config::Config, config_root: &std::path::Path) -> bool {
    !pnpm_hooks::finder::find_pnpmfiles(
        config_root,
        pnpm_package_manager::pnpmfile_selection(config),
    )
    .is_empty()
}
