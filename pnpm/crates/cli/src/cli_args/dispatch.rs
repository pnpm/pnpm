pub(super) use configuration::apply_update_config;

use super::{
    cli_command::{CliArgs, CliCommand},
    dispatch_install, dispatch_query, dispatch_script,
    install::resolve_bool_override,
    reporter::{
        DefaultReporterSetup, ReporterType, configure_color, configure_default_reporter,
        configure_max_log_level, reporter_emit,
    },
};
use crate::{
    State,
    config_deps::prepare_config,
    config_overrides::{
        ConfigOverrides, apply_registry_override, apply_state_dir_override,
        apply_store_dir_override,
    },
};

use configuration::{
    OutputOverrides, ProjectSelectors, RunAnchors, RunSetup, apply_color_override,
    apply_location_overrides, apply_output_overrides, apply_project_selectors, seed_config,
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
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

pub(crate) type CommandFuture<'a, Output = ()> =
    Pin<Box<dyn Future<Output = miette::Result<Output>> + Send + 'a>>;

/// The shared context every subcommand handler needs: the canonicalized
/// `--dir`, the derived `package.json` path, the selected reporter, the
/// `--recursive` flag, and the two lazily-loaded resources (`config` /
/// `state`) the handlers pull from on demand.
///
/// `config` and `state` are passed as `&dyn Fn` rather than eagerly loaded
/// so a handler that never needs them (`pacquet init`) doesn't pay for the
/// `.npmrc` / lockfile read, and so each call re-loads a fresh
/// `&'static mut Config` (some handlers, like `patch-commit`, deliberately
/// initialize state more than once). The closures are built in
/// [`CliArgs::run`]; their `&dyn Fn` shape matches what
/// [`super::approve_builds::ApproveBuildsArgs::prepare`] already consumes.
pub(crate) struct RunCtx<'a> {
    pub(crate) dir: &'a Path,
    /// The `--dir` as the command line gave it, or the process cwd when it
    /// gave none — pnpm's `cliOptions.dir ?? process.cwd()`. `init`
    /// scaffolds here and a non-recursive `exec` runs here, rather than at
    /// the local prefix [`Self::dir`] resolves to.
    pub(crate) cli_dir: &'a Path,
    pub(crate) manifest_path: &'a Path,
    pub(crate) reporter: ReporterType,
    pub(crate) recursive: bool,
    pub(crate) recursive_resume_from: Option<&'a str>,
    pub(crate) recursive_report_summary: bool,
    pub(crate) recursive_parallel: bool,
    /// The top-level `--if-present` spelling (`pnpm --if-present test`);
    /// merged with the flag the script subcommands declare themselves.
    pub(crate) if_present: bool,
    /// Whether a `pm` prefix (`pnpm pm clean`) forced the built-in
    /// command, so a `package.json` script of the same name must not
    /// override it. See [`crate::pm_prefix`].
    pub(crate) builtin_command_forced: bool,
    pub(crate) config: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    /// Like [`Self::config`] but anchored at the pnpm home dir instead of
    /// `--dir`, so a `-g` install can't inherit the caller project's
    /// `.npmrc` network / TLS / registry settings.
    pub(crate) global_config: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    /// Like [`Self::config`] but loaded through
    /// [`Config::current_for_self_update`], so a repo-controlled
    /// `pnpm-workspace.yaml` can only tighten the release-age policy that
    /// governs the pnpm download.
    pub(crate) config_self_update: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    pub(crate) state: &'a (dyn Fn(bool) -> miette::Result<State> + Sync),
}

impl CliArgs {
    /// Seed the process-global default-reporter state from the parsed
    /// arguments. The entry point calls this before the pre-command
    /// checks, which are the first thing that can emit — the state is set
    /// once, so whoever emits first must already see the real values. A
    /// `--color` / `--no-color` on the command line is seeded here for
    /// that reason; the `color` *setting* can only be read once the
    /// configuration is loaded, and reaches the reporter in
    /// [`Self::run`].
    /// [`Self::run`] and the install fast path call it again so a direct
    /// in-process caller is configured too; the repeat calls are no-ops.
    ///
    /// A `--dir` that cannot be canonicalized is left as given: the same
    /// path fails with a proper diagnostic in [`Self::run`], and the
    /// reporter only uses it to shorten the paths it prints.
    pub fn configure_reporter(&self) {
        if let Some(color) = self.color.or_else(|| self.no_color.then_some(ColorMode::Never)) {
            configure_color(color);
        }
        let dir = dunce::canonicalize(&self.dir).unwrap_or_else(|_| self.dir.clone());
        configure_default_reporter(&DefaultReporterSetup {
            reporter: self.effective_reporter(),
            dir: &dir,
            summary_scope: self.command.default_reporter_summary_scope(),
            reports_scope: self.command.reports_scope(self.recursive),
            hide_added_pkgs_progress: false,
            is_recursive: self.recursive,
            use_stderr: self.use_stderr || self.command.uses_stderr_reporter(),
            stream_lifecycle_output: self.stream,
            aggregate_output: self.aggregate_output,
            hide_lifecycle_prefix: self.reporter_hide_prefix,
        });
        configure_max_log_level(self.loglevel);
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
        if !self.filter.is_empty() || !self.filter_prod.is_empty() {
            return false;
        }
        let Ok(dir) = dunce::canonicalize(&self.dir) else {
            return false;
        };
        let loaded = seed_config(self.npmrc_auth_file.as_deref(), self.ignore_workspace)
            .current::<Host>(&dir);
        let Ok(mut config) = loaded else {
            return false;
        };
        config_overrides.apply(&mut config, &dir);
        config.apply_proxy_cli_overrides(
            self.https_proxy.as_deref(),
            self.http_proxy.as_deref(),
            self.no_proxy.as_deref(),
        );
        if let Some(registry) = self.registry.as_deref() {
            apply_registry_override(&mut config, registry);
        }
        if let Some(store_dir) = self.store_dir.as_deref()
            && apply_store_dir_override::<Host>(&mut config, store_dir, &dir).is_err()
        {
            return false;
        }
        if let Some(state_dir) = self.state_dir.as_deref() {
            apply_state_dir_override::<Host>(&mut config, state_dir, &dir);
        }
        install_args.lockfile_dir.apply_to(&mut config, &dir);
        self.configure_reporter();
        let emit = reporter_emit(self.effective_reporter());
        let finished = install_args.finished_via_up_to_date_fast_path(&dir, &config, emit);
        if finished {
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
        let setup = RunSetup::of(&self);
        let command = std::mem::replace(&mut self.command, CliCommand::Recursive);

        self.run_command(command, config_overrides, builtin_command_forced, &setup, &anchors)
            .await?;

        // The `Done in ...` footer covers the whole command, mirroring pnpm's
        // `pnpm:execution-time` emit in `main.ts`. Only the install-family
        // commands drive the visual reporter, so the rest stay silent.
        if setup.is_install_family {
            emit_execution_time(reporter_emit(setup.reporter), setup.started_at);
        }

        Ok(())
    }
    async fn run_command(
        &self,
        command: CliCommand,
        config_overrides: &ConfigOverrides,
        builtin_command_forced: bool,
        setup: &RunSetup,
        anchors: &RunAnchors,
    ) -> miette::Result<()> {
        // Load config anchored at `anchor`, reading `.npmrc` /
        // `pnpm-workspace.yaml` from there.
        let load_config = |anchor: &Path| -> miette::Result<&'static mut Config> {
            seed_config(self.npmrc_auth_file.as_deref(), self.ignore_workspace)
                .current::<Host>(anchor)
                .map_err(miette::Report::new)
                .wrap_err("load configuration")
                .and_then(|cfg| {
                    self.finalize_run_config(cfg, anchor, config_overrides, setup, anchors)
                })
        };
        // Resolve `.npmrc` / `pnpm-workspace.yaml` from the canonicalized
        // `--dir` rather than the process cwd, matching pnpm 11 (which
        // builds its `localPrefix` from `cliOptions.dir`, not `cwd`).
        let config = || load_config(&anchors.dir);
        let config_self_update = || -> miette::Result<&'static mut Config> {
            seed_config(self.npmrc_auth_file.as_deref(), self.ignore_workspace)
                .current_for_self_update::<Host>(&anchors.dir)
                .map_err(miette::Report::new)
                .wrap_err("load configuration")
                .and_then(|cfg| {
                    self.finalize_run_config(cfg, &anchors.dir, config_overrides, setup, anchors)
                })
        };
        // `require_lockfile` is the "this subcommand cannot run without a
        // lockfile loaded" signal, used by `State::init` to override
        // `config.lockfile=false`. Only `install --frozen-lockfile` needs
        // it today; other subcommands follow `config.lockfile`. Matches
        // pnpm's CLI: `--frozen-lockfile` is the strongest signal and
        // must not be silently dropped because `lockfile=false` was set
        // (or defaulted) in config.
        let state = |require_lockfile: bool| -> miette::Result<State> {
            State::init(anchors.manifest_path.clone(), config()?, require_lockfile)
                .wrap_err("initialize the state")
        };

        let ctx = RunCtx {
            dir: &anchors.dir,
            cli_dir: &anchors.cli_dir,
            manifest_path: &anchors.manifest_path,
            reporter: setup.reporter,
            recursive: self.recursive,
            recursive_resume_from: self.resume_from.as_deref(),
            recursive_report_summary: self.report_summary,
            recursive_parallel: self.parallel,
            if_present: self.if_present,
            builtin_command_forced,
            config: &config,
            global_config: &|| load_config(&anchors.global_config),
            config_self_update: &config_self_update,
            state: &state,
        };
        exit_on_json_error(run_routed_command(command, &ctx).await, setup.print_json_errors)
    }

    fn finalize_run_config(
        &self,
        mut cfg: Config,
        anchor: &Path,
        config_overrides: &ConfigOverrides,
        setup: &RunSetup,
        anchors: &RunAnchors,
    ) -> miette::Result<&'static mut Config> {
        config_overrides.apply(&mut cfg, anchor);
        apply_color_override(&mut cfg, self.color, self.no_color);
        if cfg.ci {
            pnpm_default_reporter::force_append_only();
        }
        cfg.apply_proxy_cli_overrides(
            self.https_proxy.as_deref(),
            self.http_proxy.as_deref(),
            self.no_proxy.as_deref(),
        );
        apply_location_overrides(
            &mut cfg,
            anchor,
            self.registry.as_deref(),
            self.store_dir.as_deref(),
            self.state_dir.as_deref(),
        )?;
        apply_project_selectors(
            &mut cfg,
            &ProjectSelectors {
                recursive: self.recursive,
                recursive_by_default_command: setup.recursive_by_default,
                filter: &self.filter,
                filter_prod: &self.filter_prod,
                workspace_root: self.workspace_root,
                fail_if_no_match: self.fail_if_no_match,
            },
        );
        self.apply_run_output_config(&mut cfg);
        self.configure_run_reporter(&cfg, setup, anchors);
        Ok(Config::leak(cfg))
    }

    fn configure_run_reporter(&self, cfg: &Config, setup: &RunSetup, anchors: &RunAnchors) {
        configure_default_reporter(&DefaultReporterSetup {
            reporter: setup.reporter,
            dir: &anchors.dir,
            summary_scope: setup.summary_scope,
            reports_scope: setup.reports_scope,
            hide_added_pkgs_progress: false,
            is_recursive: self.recursive,
            use_stderr: cfg.use_stderr || setup.uses_stderr_reporter,
            stream_lifecycle_output: cfg.stream,
            aggregate_output: cfg.aggregate_output,
            hide_lifecycle_prefix: cfg.reporter_hide_prefix.unwrap_or(false),
        });
    }

    fn apply_run_output_config(&self, cfg: &mut Config) {
        cfg.bail = resolve_bool_override(self.bail, self.no_bail, cfg.bail);
        cfg.stream |= self.stream;
        cfg.aggregate_output |= self.aggregate_output;
        cfg.use_stderr |= self.use_stderr;
        cfg.sort = resolve_bool_override(self.sort, self.no_sort, cfg.sort);
        cfg.reverse = resolve_bool_override(self.reverse, self.no_reverse, cfg.reverse);
        cfg.include_workspace_root = resolve_bool_override(
            self.include_workspace_root,
            self.no_include_workspace_root,
            cfg.include_workspace_root,
        );
        apply_output_overrides(
            cfg,
            &OutputOverrides {
                reporter_hide_prefix: self.reporter_hide_prefix,
                no_reporter_hide_prefix: self.no_reporter_hide_prefix,
                workspace_packages: &self.workspace_packages,
                test_pattern: &self.test_pattern,
                changed_files_ignore_pattern: &self.changed_files_ignore_pattern,
                workspace_concurrency: self.workspace_concurrency,
            },
        );
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
