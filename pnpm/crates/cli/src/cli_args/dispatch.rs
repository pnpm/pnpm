pub(crate) use context::{
    CommandFuture, CommandLoaders, CommandLocations, RunCtx, WorkspaceInvocation,
};

pub(super) use super::pre_command::{apply_cli_layers, seed_config};
pub(super) use configuration::apply_update_config;

use super::{
    cli_command::{CliArgs, CliCommand},
    config_warnings::{ConfigWarnings, warn_shared_workspace_lockfile_outside_workspace},
    dispatch_install, dispatch_query, dispatch_script,
    install::{InstallArgs, resolve_bool_override},
    pre_command::Invocation,
    reporter::{
        DefaultReporterSetup, ReporterType, configure_color, configure_default_reporter,
        configure_max_log_level, reporter_emit,
    },
};
use crate::{
    cli_args::reporter::{select_reporter, selected_reporter},
    config_deps::prepare_config,
    config_overrides::ConfigOverrides,
};
use configuration::{
    ConfigTarget, ProjectSelectors, RunAnchors, RunSetup, StoreUse, apply_project_selectors,
    apply_run_output_config, warn_fast_path_config,
};
use context::take_preloaded_config;
use miette::{Context, IntoDiagnostic};
use pnpm_config::{ColorMode, Config, ConfigLoadFailure, Host, default_pnpm_home_dir};
use pnpm_default_reporter::SummaryScope;
use pnpm_network_web_auth::OtpNonInteractiveError;
use pnpm_reporter::{ExecutionTimeLog, LogEvent, LogLevel};
use routing::{
    emit_execution_time, exit_on_json_error, now_millis, prints_json_errors, run_routed_command,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
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

    /// Report the warnings of the fast path's configuration. `None` when one
    /// fails the command, which the full path then reports.
    fn report_fast_path_warnings(&self, config: &Config, dir: &Path) -> Option<()> {
        let warnings = ConfigWarnings::of(&self.command);
        if warnings.fail(config, dir) {
            return None;
        }
        warnings.report(config, dir).ok()
    }

    fn prepare_fast_path_config(
        &self,
        invocation: &Invocation,
        config_overrides: &ConfigOverrides,
        install_args: &InstallArgs,
        preloaded_config: &mut Option<Result<Config, ConfigLoadFailure>>,
    ) -> Option<(PathBuf, Config)> {
        if !self.workspace.selection.filter.is_empty()
            || !self.workspace.selection.filter_prod.is_empty()
        {
            return None;
        }
        let dir = dunce::canonicalize(&self.paths.dir).ok()?;
        let mut config =
            self.take_or_load_fast_path_config(&dir, config_overrides, preloaded_config)?;
        if self.report_fast_path_warnings(&config, &dir).is_none()
            || apply_cli_layers(&mut config, invocation, config_overrides, &dir).is_err()
        {
            *preloaded_config = Some(Ok(config));
            return None;
        }
        install_args.lockfile.directory.apply_to(&mut config, &dir);
        config.frozen_store = resolve_bool_override(
            install_args.materialization.frozen_store,
            install_args.materialization.no_frozen_store,
            config.frozen_store,
        );
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
    /// with [`Self::run`], reusing any already-loaded configuration.
    pub(crate) fn finished_via_install_fast_path(
        &self,
        invocation: &Invocation,
        config_overrides: &ConfigOverrides,
        preloaded_config: &mut Option<Result<Config, ConfigLoadFailure>>,
    ) -> bool {
        let started_at = now_millis();
        let CliCommand::Install(install_args) = &self.command else {
            return false;
        };
        let prepared = {
            let _span = tracing::debug_span!(target: "pacquet::install", "load_config").entered();
            self.prepare_fast_path_config(
                invocation,
                config_overrides,
                install_args,
                preloaded_config,
            )
        };
        let Some((dir, config)) = prepared else {
            return false;
        };
        let emit =
            reporter_emit(self.effective_reporter_with_config(config.loglevel, config.reporter));
        let finished = install_args.finished_via_up_to_date_fast_path(&dir, &config, emit);
        if finished {
            warn_fast_path_config(config_overrides, &config);
            emit_execution_time(emit, started_at);
        } else {
            *preloaded_config = Some(Ok(config));
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
    pub(crate) async fn run(
        mut self,
        invocation: &Invocation,
        config_overrides: &ConfigOverrides,
        builtin_command_forced: bool,
        preloaded_config: Option<Result<Config, ConfigLoadFailure>>,
    ) -> miette::Result<()> {
        if self.run_completion_if_requested()? {
            return Ok(());
        }
        self.configure_reporter();
        self.check_global_conflicts(config_overrides)?;

        let anchors = RunAnchors::resolve(&self)?;
        select_reporter(self.effective_reporter());
        let setup = RunSetup::of(&self);
        let command = std::mem::replace(&mut self.command, CliCommand::Recursive);

        let builtin_replaced_by_script = self.run_command(
            command,
            invocation,
            config_overrides,
            builtin_command_forced,
            &setup,
            &anchors,
            preloaded_config,
        )
        .await?;

        // The `Done in ...` footer covers the whole command, mirroring pnpm's
        // `pnpm:execution-time` emit in `main.ts`. Only the install-family
        // commands drive the visual reporter, so the rest stay silent.
        if setup.is_install_family && !builtin_replaced_by_script {
            emit_execution_time(reporter_emit(selected_reporter()), setup.started_at);
        }

        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "command execution threads parsed CLI state and preloaded config into loaders"
    )]
    async fn run_command(
        &self,
        command: CliCommand,
        invocation: &Invocation,
        config_overrides: &ConfigOverrides,
        builtin_command_forced: bool,
        setup: &RunSetup,
        anchors: &RunAnchors,
        preloaded_config: Option<Result<Config, ConfigLoadFailure>>,
    ) -> miette::Result<bool> {
        let target_template =
            (command.npm_command_name(), StoreUse::of(&command), ConfigWarnings::of(&command));
        let preloaded_config = Mutex::new(preloaded_config);
        let load_config = |anchor: &Path, is_global: bool| {
            let target = ConfigTarget {
                anchor,
                is_global,
                store_use: target_template.1,
                warnings: target_template.2,
            };
            self.load_and_finalize_config(
                &target,
                invocation,
                config_overrides,
                setup,
                anchors,
                target_template.0,
                take_preloaded_config(&preloaded_config, anchor, is_global, &anchors.dir),
            )
        };
        // Resolve `.npmrc` / `pnpm-workspace.yaml` from the canonicalized
        // `--dir` rather than the process cwd, matching pnpm 11 (which
        // builds its `localPrefix` from `cliOptions.dir`, not `cwd`).
        let config = || load_config(&anchors.dir, false);
        let config_self_update =
            || self.load_self_update_config(invocation, config_overrides, setup, anchors);
        let builtin_replaced_by_script = AtomicBool::new(false);
        let ctx = RunCtx {
            reporter_flags: self.reporter_flags(),
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

    #[expect(
        clippy::too_many_arguments,
        reason = "combines target options, invocation layers, and preloaded config into a static Config"
    )]
    fn load_and_finalize_config(
        &self,
        target: &ConfigTarget<'_>,
        invocation: &Invocation,
        config_overrides: &ConfigOverrides,
        setup: &RunSetup,
        anchors: &RunAnchors,
        npm_command: &'static str,
        preloaded: Option<Result<Config, ConfigLoadFailure>>,
    ) -> miette::Result<&'static mut Config> {
        let ConfigTarget {
            anchor,
            is_global,
            store_use,
            warnings,
        } = *target;
        let cfg = match preloaded {
            Some(Ok(cfg)) if !cfg.skip_store_dir_resolution => cfg,
            Some(Err(failure)) => {
                warnings.report_failed_load(&failure.warnings);
                return Err(miette::Report::new(failure.error)).wrap_err("load configuration");
            }
            Some(Ok(_)) | None => store_use.load(|place_store| {
                self.load_config_at(anchor, place_store, config_overrides, warnings)
            })?,
        };
        warnings.report(&cfg, anchor)?;
        let config = self.finalize_run_config(
            cfg,
            anchor,
            is_global,
            invocation,
            config_overrides,
            setup,
            anchors,
        )?;
        // npm names the running command for scripts in `npm_command`, and pnpm 11
        // sets the same variable, so scripts that branch on it behave alike
        // whichever CLI runs them. This map is what every script-spawning site
        // reads.
        config.extra_env.insert("npm_command".to_string(), npm_command.to_string());
        Ok(config)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "layers CLI flags, project selectors, and reporter setup onto the loaded Config"
    )]
    fn finalize_run_config(
        &self,
        mut cfg: Config,
        anchor: &Path,
        is_global: bool,
        invocation: &Invocation,
        config_overrides: &ConfigOverrides,
        setup: &RunSetup,
        anchors: &RunAnchors,
    ) -> miette::Result<&'static mut Config> {
        apply_cli_layers(&mut cfg, invocation, config_overrides, anchor)?;
        if !is_global {
            warn_shared_workspace_lockfile_outside_workspace(
                config_overrides.shared_workspace_lockfile(),
                cfg.workspace_dir.as_deref(),
            );
        }
        apply_project_selectors(
            &mut cfg,
            &ProjectSelectors {
                recursive: self.workspace.recursive,
                recursive_from_command_line: self.workspace.recursive_from_command_line,
                recursive_by_default_command: setup.recursive_by_default,
                filter: &self.workspace.selection.filter,
                filter_prod: &self.workspace.selection.filter_prod,
                workspace_root: self.workspace.selection.workspace_root,
            },
        );
        apply_run_output_config(self, &mut cfg);
        let reporter = self.effective_reporter_with_config(cfg.loglevel, cfg.reporter);
        select_reporter(reporter);
        self.configure_run_reporter(&cfg, reporter, setup, anchors);
        Ok(Config::leak(cfg))
    }
}

#[cfg(test)]
mod tests;

mod routing;

mod configuration;

mod context;
