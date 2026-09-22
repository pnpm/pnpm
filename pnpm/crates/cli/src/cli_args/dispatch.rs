pub(crate) use context::{
    CommandFuture, CommandLoaders, CommandLocations, RunCtx, WorkspaceInvocation,
};

pub(super) use configuration::{apply_update_config, seed_config};

use super::{
    cli_command::{CliArgs, CliCommand},
    config_warnings::warn_shared_workspace_lockfile_outside_workspace,
    dispatch_install, dispatch_query, dispatch_script,
    install::{InstallArgs, resolve_bool_override},
    reporter::{
        DefaultReporterSetup, ReporterType, configure_color, configure_default_reporter,
        configure_max_log_level, reporter_emit,
    },
};
use crate::{
    config_deps::prepare_config,
    config_overrides::{ConfigOverrides, apply_state_dir_override, apply_store_dir_override},
};

use configuration::{
    ProjectSelectors, RunAnchors, RunSetup, apply_color_override, apply_location_overrides,
    apply_project_selectors, apply_run_output_config, warn_fast_path_config,
};
use miette::{Context, IntoDiagnostic};
use pnpm_config::{ColorMode, Config, Host, default_pnpm_home_dir};
use pnpm_default_reporter::{DefaultReporter, SummaryScope};
use pnpm_network_web_auth::OtpNonInteractiveError;
use pnpm_reporter::{ExecutionTimeLog, LogEvent, LogLevel, NdjsonReporter, SilentReporter};
use routing::{
    emit_execution_time, now_millis, print_json_error, prints_json_errors, run_routed_command,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

impl CliArgs {
    /// Seed the process-global default-reporter state from the parsed
    /// arguments. The entry point calls this before the pre-command
    /// checks, which are the first thing that can emit — the state is set
    /// once, so whoever emits first must already see the real values. A
    /// `--color` / `--no-color` on the command line is seeded here for
    /// that reason; the `color` *setting* can only be read once the
    /// configuration is loaded, and reaches the reporter in
    /// [`Self::run`]. [`Self::run`] and the install fast path call it
    /// again so a direct in-process caller is configured too; the repeat
    /// calls leave the once-set state alone and only re-seed `progress`,
    /// which the loaded configuration may still turn off.
    ///
    /// A `--dir` that cannot be canonicalized is left as given: the same
    /// path fails with a proper diagnostic in [`Self::run`], and the
    /// reporter only uses it to shorten the paths it prints.
    pub fn configure_reporter(&self) {
        if let Some(color) = self.output.presentation.color.or_else(|| {
            self.output.presentation.no_color.then_some(ColorMode::Never)
        }) {
            configure_color(color);
        }
        let dir = dunce::canonicalize(&self.paths.dir)
            .unwrap_or_else(|_| self.paths.dir.clone());
        pnpm_default_reporter::set_progress(self.progress_enabled(true));
        configure_default_reporter(&DefaultReporterSetup {
            reporter: self.effective_reporter(),
            dir: &dir,
            summary_scope: self.command.default_reporter_summary_scope(),
            reports_scope: self.command.reports_scope(self.workspace.recursive),
            hide_added_pkgs_progress: false,
            is_recursive: self.workspace.recursive,
            lifecycle: crate::cli_args::reporter::LifecycleReporterSetup {
                use_stderr: self.output.lifecycle.use_stderr || self.command.uses_stderr_reporter(),
                stream_output: self.output.lifecycle.stream,
                aggregate_output: self.output.lifecycle.aggregate_output,
                hide_prefix: self.output.lifecycle.hide_prefix,
            },
        });
        configure_max_log_level(self.output.presentation.loglevel);
    }

    /// Resolve whether progress is rendered: `--progress` /
    /// `--no-progress` over `config`, which is the loaded `progress`
    /// setting, or its default where the configuration is not read yet.
    fn progress_enabled(&self, config: bool) -> bool {
        resolve_bool_override(
            self.output.presentation.progress,
            self.output.presentation.no_progress,
            config,
        )
    }

    pub fn run_completion_if_requested(&self) -> miette::Result<bool> {
        match &self.command {
            CliCommand::Completion(args) => {
                args.run()?;
                Ok(true)
            }
            CliCommand::CompletionServer(args) => {
                args.run()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn prepare_fast_path_config(
        &self,
        config_overrides: &ConfigOverrides,
        install_args: &InstallArgs,
    ) -> Option<(PathBuf, Config)> {
        if !self.workspace.selection.filter.is_empty()
            || !self.workspace.selection.filter_prod.is_empty()
        {
            return None;
        }
        let dir = dunce::canonicalize(&self.paths.dir).ok()?;
        let mut config =
            seed_config(self.paths.npmrc_auth_file.as_deref(), self.paths.ignore_workspace)
                .current::<Host>(&dir)
                .ok()?;
        config_overrides.apply(&mut config, &dir);
        self.network.apply(&mut config);
        if let Some(store_dir) = self.paths.store_dir.as_deref()
            && apply_store_dir_override::<Host>(&mut config, store_dir, &dir).is_err()
        {
            return None;
        }
        if let Some(state_dir) = self.paths.state_dir.as_deref() {
            apply_state_dir_override::<Host>(&mut config, state_dir, &dir);
        }
        install_args.lockfile.directory.apply_to(&mut config, &dir);
        config.progress = self.progress_enabled(config.progress);
        self.configure_reporter();
        if self.output.presentation.loglevel.is_none()
            && let Some(config_loglevel) = config.loglevel
        {
            configure_max_log_level(Some(config_loglevel.into()));
        }
        pnpm_default_reporter::set_progress(config.progress);
        Some((dir, config))
    }

    /// Try to finish `pacquet install` synchronously through the
    /// repeat-install fast path, before the caller builds the async
    /// runtime. `true` means the install completed (the "Already up to
    /// date" events were emitted); `false` means undecided — proceed
    /// with [`Self::run`], which loads its own config and re-runs the
    /// same check.
    ///
    /// Mirrors the install arm of [`Self::run`]'s dispatch: the same
    /// canonicalized `--dir`, the same config seed (`--npmrc-auth-file`
    /// and `--ignore-workspace`), the same `--config.<key>` overrides. A
    /// config loaded any other way would answer for a different project.
    ///
    /// Filtered installs always take the full path; an unfiltered
    /// recursive one does not — inside a workspace every install is
    /// recursive, and the up-to-date check speaks for the whole
    /// workspace.
    pub fn finished_via_install_fast_path(&self, config_overrides: &ConfigOverrides) -> bool {
        let started_at = now_millis();
        let CliCommand::Install(install_args) = &self.command else {
            return false;
        };
        let Some((dir, config)) = self.prepare_fast_path_config(config_overrides, install_args)
        else {
            return false;
        };
        let emit = reporter_emit(self.effective_reporter_with_config(config.loglevel));
        let finished = install_args.finished_via_up_to_date_fast_path(&dir, &config, emit);
        if finished {
            warn_fast_path_config(config_overrides, &config);
            emit_execution_time(emit, started_at);
        }
        finished
    }

    /// Execute the command. `config_overrides` carries `--config.<key>=<value>`
    /// tokens already stripped from argv by [`ConfigOverrides::extract`];
    /// they're layered on top of `.npmrc` / `pnpm-workspace.yaml` whenever
    /// `Config` is loaded, mirroring pnpm 11's
    /// "CLI > yaml > .npmrc > defaults" precedence. `builtin_command_forced`
    /// carries the `pm` prefix stripped from argv by
    /// [`crate::pm_prefix::strip_prefix`].
    pub async fn run(
        mut self,
        config_overrides: &ConfigOverrides,
        builtin_command_forced: bool,
    ) -> miette::Result<()> {
        if self.run_completion_if_requested()? {
            return Ok(());
        }
        self.configure_reporter();

        let anchors = RunAnchors::resolve(&self)?;
        let effective_reporter = AtomicU8::new(self.effective_reporter() as u8);
        let setup = RunSetup::of(&self, &effective_reporter);
        let command = std::mem::replace(&mut self.command, CliCommand::Recursive);

        let builtin_replaced_by_script =
            self.run_command(command, config_overrides, builtin_command_forced, &setup, &anchors)
                .await?;

        // The `Done in ...` footer covers the whole command, mirroring pnpm's
        // `pnpm:execution-time` emit in `main.ts`. Only the install-family
        // commands drive the visual reporter, so the rest stay silent.
        if setup.is_install_family && !builtin_replaced_by_script {
            let final_reporter: ReporterType = effective_reporter.load(Ordering::Relaxed).into();
            emit_execution_time(reporter_emit(final_reporter), setup.started_at);
        }

        Ok(())
    }
    async fn run_command(
        &self,
        command: CliCommand,
        config_overrides: &ConfigOverrides,
        builtin_command_forced: bool,
        setup: &RunSetup<'_>,
        anchors: &RunAnchors,
    ) -> miette::Result<bool> {
        // Load config anchored at `anchor`, reading `.npmrc` /
        // `pnpm-workspace.yaml` from there.
        let load_config = |anchor: &Path, is_global: bool| {
            self.load_and_finalize_config(anchor, is_global, config_overrides, setup, anchors)
        };
        // Resolve `.npmrc` / `pnpm-workspace.yaml` from the canonicalized
        // `--dir` rather than the process cwd, matching pnpm 11 (which
        // builds its `localPrefix` from `cliOptions.dir`, not `cwd`).
        let config = || load_config(&anchors.dir, false);
        let config_self_update = || -> miette::Result<&'static mut Config> {
            seed_config(self.paths.npmrc_auth_file.as_deref(), self.paths.ignore_workspace)
                .current_for_self_update::<Host>(&anchors.dir)
                .map_err(miette::Report::new)
                .wrap_err("load configuration")
                .and_then(|cfg| {
                    self.finalize_run_config(
                        cfg,
                        &anchors.dir,
                        false,
                        config_overrides,
                        setup,
                        anchors,
                    )
                })
        };
        let builtin_replaced_by_script = AtomicBool::new(false);
        let ctx = RunCtx {
            effective_reporter: setup.effective_reporter,
            builtin_command_forced,
            builtin_replaced_by_script: &builtin_replaced_by_script,
            locations: CommandLocations::from(anchors),
            workspace: WorkspaceInvocation::from(&self.workspace),
            loaders: CommandLoaders {
                config: &config,
                global_config: &|| load_config(&anchors.global_config, true),
                config_self_update: &config_self_update,
            },
        };
        exit_on_json_error(run_routed_command(command, &ctx).await, setup.print_json_errors)?;
        Ok(builtin_replaced_by_script.load(Ordering::Relaxed))
    }

    fn load_and_finalize_config(
        &self,
        anchor: &Path,
        is_global: bool,
        config_overrides: &ConfigOverrides,
        setup: &RunSetup<'_>,
        anchors: &RunAnchors,
    ) -> miette::Result<&'static mut Config> {
        seed_config(self.paths.npmrc_auth_file.as_deref(), self.paths.ignore_workspace)
            .current::<Host>(anchor)
            .map_err(miette::Report::new)
            .wrap_err("load configuration")
            .and_then(|cfg| {
                self.finalize_run_config(cfg, anchor, is_global, config_overrides, setup, anchors)
            })
    }

    fn finalize_run_config(
        &self,
        mut cfg: Config,
        anchor: &Path,
        is_global: bool,
        config_overrides: &ConfigOverrides,
        setup: &RunSetup<'_>,
        anchors: &RunAnchors,
    ) -> miette::Result<&'static mut Config> {
        config_overrides.apply(&mut cfg, anchor);
        if !is_global {
            warn_shared_workspace_lockfile_outside_workspace(
                config_overrides.shared_workspace_lockfile(),
                cfg.workspace_dir.as_deref(),
            );
        }
        apply_color_override(
            &mut cfg,
            self.output.presentation.color,
            self.output.presentation.no_color,
        );
        if cfg.ci {
            pnpm_default_reporter::force_append_only();
        }
        self.network.apply(&mut cfg);
        apply_location_overrides(
            &mut cfg,
            anchor,
            None,
            self.paths.store_dir.as_deref(),
            self.paths.state_dir.as_deref(),
        )?;
        apply_project_selectors(
            &mut cfg,
            &ProjectSelectors {
                recursive: self.workspace.recursive,
                recursive_by_default_command: setup.recursive_by_default,
                filter: &self.workspace.selection.filter,
                filter_prod: &self.workspace.selection.filter_prod,
                workspace_root: self.workspace.selection.workspace_root,
                fail_if_no_match: self.workspace.selection.fail_if_no_match,
            },
        );
        apply_run_output_config(self, &mut cfg);
        let reporter = self.effective_reporter_with_config(cfg.loglevel);
        setup.effective_reporter.store(reporter as u8, Ordering::Relaxed);
        self.configure_run_reporter(&cfg, reporter, setup, anchors);
        Ok(Config::leak(cfg))
    }

    fn configure_run_reporter(
        &self,
        cfg: &Config,
        reporter: ReporterType,
        setup: &RunSetup,
        anchors: &RunAnchors,
    ) {
        pnpm_default_reporter::set_progress(cfg.progress);
        if self.output.presentation.loglevel.is_none()
            && let Some(config_loglevel) = cfg.loglevel
        {
            configure_max_log_level(Some(config_loglevel.into()));
        }
        configure_default_reporter(&DefaultReporterSetup {
            reporter,
            dir: &anchors.dir,
            summary_scope: setup.summary_scope,
            reports_scope: setup.reports_scope,
            hide_added_pkgs_progress: false,
            is_recursive: self.workspace.recursive,
            lifecycle: crate::cli_args::reporter::LifecycleReporterSetup {
                use_stderr: cfg.use_stderr || setup.uses_stderr_reporter,
                stream_output: cfg.stream,
                aggregate_output: cfg.aggregate_output,
                hide_prefix: cfg.reporter_hide_prefix.unwrap_or(false),
            },
        });
    }
}

/// A JSON-mode command reports its own failure on stdout and exits, so the
/// human-readable miette report never renders.
fn exit_on_json_error(result: miette::Result<()>, print_json_errors: bool) -> miette::Result<()> {
    if let Err(error) = &result
        && print_json_errors
    {
        print_json_error(error);
        #[expect(clippy::exit, reason = "a JSON-mode command exits non-zero after its own report")]
        std::process::exit(1);
    }
    result
}

#[cfg(test)]
mod tests;

mod routing;

mod configuration;

mod context;
