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
use miette::{Context, IntoDiagnostic};
use pnpm_config::{ColorMode, Config, Host, default_pnpm_home_dir};
use pnpm_default_reporter::{DefaultReporter, SummaryScope};
use pnpm_network_web_auth::OtpNonInteractiveError;
use pnpm_reporter::{ExecutionTimeLog, LogEvent, LogLevel, NdjsonReporter, SilentReporter};
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
    /// canonicalized `--dir`, the same config layering (`.npmrc` auth
    /// file seed + `--config.<key>` overrides). Filtered installs always
    /// take the full path; an unfiltered recursive one does not — inside
    /// a workspace every install is recursive, and the up-to-date check
    /// speaks for the whole workspace.
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
        let loaded = Config { npmrc_auth_file: self.npmrc_auth_file.clone(), ..Config::default() }
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

/// The directories a run is anchored at.
struct RunAnchors {
    /// The canonicalized `--dir`.
    dir: PathBuf,
    cli_dir: PathBuf,
    manifest_path: PathBuf,
    /// Where a `-g` install loads its config from. A `-g` install is
    /// isolated from the caller's project: pnpm runs it with `cwd` = the
    /// pnpm home dir, so a project `.npmrc` cannot influence the network /
    /// TLS / registry decisions of a *global* install. Mirror that by
    /// anchoring the global-install config at the pnpm home (resolved from
    /// `PNPM_HOME` / platform defaults, never the project). Falls back to
    /// the `--dir` anchor when the home can't be determined — the global
    /// command then fails at the missing-global-bin-dir check regardless.
    global_config: PathBuf,
}

impl RunAnchors {
    /// Canonicalize `--dir` so the bunyan-envelope `prefix` emitted by the
    /// reporter is the same absolute, symlink-resolved path that
    /// `@pnpm/cli.default-reporter` derives via `process.cwd()`. Without
    /// this, a default `--dir=.` leaves `prefix` as `"."`, the reporter
    /// never matches it against its `cwd`, and every progress / stats line
    /// gets a redundant `.` path prefix prepended. The resolved path
    /// becomes `config.dir` (used as the install `lockfileDir`, threaded
    /// into every event's `prefix`).
    fn resolve(args: &CliArgs) -> miette::Result<Self> {
        let dir = dunce::canonicalize(&args.dir).into_diagnostic().wrap_err_with(|| {
            format!("canonicalizing the `--dir` argument: {}", args.dir.display())
        })?;
        let cli_dir = if args.dir_from_command_line {
            dir.clone()
        } else {
            std::env::current_dir().and_then(dunce::canonicalize).unwrap_or_else(|_| dir.clone())
        };
        let manifest_path = dir.join("package.json");
        let global_config = default_pnpm_home_dir::<Host>().unwrap_or_else(|| dir.clone());
        Ok(RunAnchors { dir, cli_dir, manifest_path, global_config })
    }
}

/// What the command line settles before any config is loaded, and when
/// the run began.
struct RunSetup {
    started_at: u128,
    reporter: ReporterType,
    is_install_family: bool,
    print_json_errors: bool,
    recursive_by_default: bool,
    summary_scope: SummaryScope,
    reports_scope: bool,
    uses_stderr_reporter: bool,
}

impl RunSetup {
    fn of(args: &CliArgs) -> Self {
        RunSetup {
            started_at: now_millis(),
            reporter: args.effective_reporter(),
            is_install_family: matches!(
                &args.command,
                CliCommand::Add(_)
                    | CliCommand::Update(_)
                    | CliCommand::Remove(_)
                    | CliCommand::Install(_)
                    | CliCommand::InstallTest(_)
                    | CliCommand::Ci(_)
                    | CliCommand::Dlx(_)
                    | CliCommand::Link(_)
                    | CliCommand::Import(_)
                    | CliCommand::Dedupe(_)
                    | CliCommand::Deploy(_)
                    | CliCommand::Prune(_)
                    | CliCommand::Fetch(_)
                    | CliCommand::Unlink(_)
                    | CliCommand::Create(_)
                    | CliCommand::Runtime(_)
                    // `rebuild` drives the frozen-install pipeline and emits
                    // the same progress events, so it shares the `Done in ...`
                    // footer.
                    | CliCommand::Rebuild(_)
                    | CliCommand::PatchCommit(_)
                    | CliCommand::PatchRemove(_),
            ),
            print_json_errors: prints_json_errors(&args.command),
            recursive_by_default: args.command.recursive_by_default(),
            summary_scope: args.command.default_reporter_summary_scope(),
            reports_scope: args.command.reports_scope(args.recursive),
            uses_stderr_reporter: args.command.uses_stderr_reporter(),
        }
    }
}

/// The config every load starts from. `npmrc_auth_file` is seeded from the
/// CLI flag before `current()` reads `.npmrc`, so the override redirects the
/// user-level read. Mirrors pnpm's `--npmrc-auth-file`. Production callers
/// turbofish `Host` explicitly so the dependency-injection plumbing is
/// visible at the call site. See
/// [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339).
fn seed_config(npmrc_auth_file: Option<&Path>, ignore_workspace: bool) -> Config {
    Config {
        npmrc_auth_file: npmrc_auth_file.map(Path::to_path_buf),
        ignore_workspace,
        ..Config::default()
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

/// `--color` wins over `--no-color`, and both over the configured value.
fn apply_color_override(cfg: &mut Config, color: Option<pnpm_config::ColorMode>, no_color: bool) {
    cfg.color = match (color, no_color) {
        (Some(color), _) => color,
        (None, true) => pnpm_config::ColorMode::Never,
        (None, false) => cfg.color,
    };
    configure_color(cfg.color);
}

/// The CLI overrides that redirect where pnpm reads and writes.
fn apply_location_overrides(
    cfg: &mut Config,
    anchor: &Path,
    registry: Option<&str>,
    store_dir: Option<&Path>,
    state_dir: Option<&Path>,
) -> miette::Result<()> {
    if let Some(registry) = registry {
        apply_registry_override(cfg, registry);
    }
    if let Some(store_dir) = store_dir {
        apply_store_dir_override::<Host>(cfg, store_dir, anchor)?;
    }
    if let Some(state_dir) = state_dir {
        apply_state_dir_override::<Host>(cfg, state_dir, anchor);
    }
    Ok(())
}

/// `--recursive` / `--filter` / `--filter-prod` / `--workspace-root` /
/// `--fail-if-no-match` are CLI-only upstream (not `.npmrc` / yaml
/// keys), so the global flags are threaded in here. Mirrors pnpm's
/// `Config.recursive` / `.filter` / `.filterProd` / `.workspaceRoot` /
/// `.failIfNoMatch`.
struct ProjectSelectors<'a> {
    recursive: bool,
    recursive_by_default_command: bool,
    filter: &'a [String],
    filter_prod: &'a [String],
    workspace_root: bool,
    fail_if_no_match: bool,
}

fn apply_project_selectors(cfg: &mut Config, selectors: &ProjectSelectors<'_>) {
    cfg.recursive = selectors.recursive;
    cfg.filter = selectors.filter.to_vec();
    cfg.filter_prod = selectors.filter_prod.to_vec();
    if selectors.recursive_by_default_command
        && cfg.recursive
        && !cfg.recursive_install
        && cfg.filter.is_empty()
        && cfg.filter_prod.is_empty()
    {
        cfg.filter.push("{.}...".to_string());
    }
    cfg.workspace_root = selectors.workspace_root;
    cfg.fail_if_no_match = selectors.fail_if_no_match;
}

/// The CLI flags that shape what the command prints and how much of it
/// runs at once.
struct OutputOverrides<'a> {
    reporter_hide_prefix: bool,
    no_reporter_hide_prefix: bool,
    workspace_packages: &'a [String],
    test_pattern: &'a [String],
    changed_files_ignore_pattern: &'a [String],
    workspace_concurrency: Option<i32>,
}

fn apply_output_overrides(cfg: &mut Config, overrides: &OutputOverrides<'_>) {
    if overrides.reporter_hide_prefix || overrides.no_reporter_hide_prefix {
        cfg.reporter_hide_prefix = Some(overrides.reporter_hide_prefix);
    }
    if !overrides.workspace_packages.is_empty() {
        cfg.workspace_package_patterns = Some(overrides.workspace_packages.to_vec());
    }
    // Unlike the CLI-only selectors, these two are genuine config keys —
    // the flag overrides yaml / env only when actually given.
    if !overrides.test_pattern.is_empty() {
        cfg.test_pattern = overrides.test_pattern.to_vec();
    }
    if !overrides.changed_files_ignore_pattern.is_empty() {
        cfg.changed_files_ignore_pattern = overrides.changed_files_ignore_pattern.to_vec();
    }
    if let Some(workspace_concurrency) = overrides.workspace_concurrency {
        cfg.workspace_concurrency =
            pnpm_config::resolve_child_concurrency(Some(workspace_concurrency));
    }
}

/// Route the command and await it, so a routing failure and a command
/// failure reach the caller the same way.
async fn run_routed_command(command: CliCommand, ctx: &RunCtx<'_>) -> miette::Result<()> {
    route(command, ctx)?.await
}

/// Install the project's config dependencies and apply their `updateConfig`
/// pnpmfile hooks to `config` before a command spawns anything, so a hook's
/// settings — `extraEnv` and `extraBinPaths` among them — reach the child
/// processes the command starts. pnpm applies them once per invocation,
/// whatever the command.
///
/// Every [`RunCtx::config`] call yields a fresh `Config`, so the pass has to
/// run on the instance the handler goes on to use — it cannot be hoisted ahead
/// of [`route`]. The install family and pack/publish apply the same pass at
/// their own entry points, where they already hold that instance.
pub(super) async fn apply_update_config(
    config: &mut Config,
    dir: &Path,
    reporter: ReporterType,
) -> miette::Result<()> {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            prepare_config::<DefaultReporter>(config, dir).await?
        }
        ReporterType::Ndjson => prepare_config::<NdjsonReporter>(config, dir).await?,
        ReporterType::Silent => prepare_config::<SilentReporter>(config, dir).await?,
    };
    Ok(())
}

/// Route a parsed [`CliCommand`] to its handler. The per-command logic lives
/// in the `dispatch_install` / `dispatch_query` / `dispatch_script` modules,
/// grouped by what the command does (mutate the install graph, read-only
/// query, or run a `package.json` script); this match is only the wiring.
///
/// `completion` / `completion-server` are handled before configuration in
/// [`CliArgs::run_completion_if_requested`], so they are unreachable here.
fn route<'a>(command: CliCommand, ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Add(args) => dispatch_install::add(ctx, args),
        CliCommand::Install(args) => dispatch_install::install(ctx, args),
        CliCommand::InstallTest(args) => dispatch_install::install_test(ctx, args),
        CliCommand::Ci(args) => dispatch_install::ci(ctx, args),
        CliCommand::Pipeline(args) => dispatch_install::pipeline(ctx, args),
        CliCommand::Update(args) => dispatch_install::update(ctx, args),
        CliCommand::Rebuild(args) => dispatch_install::rebuild(ctx, args),
        CliCommand::Remove(args) => dispatch_install::remove(ctx, args),
        CliCommand::Patch(args) => dispatch_install::patch(ctx, args),
        CliCommand::PatchCommit(args) => dispatch_install::patch_commit(ctx, args),
        CliCommand::PatchRemove(args) => dispatch_install::patch_remove(ctx, args),
        CliCommand::Dlx(args) => dispatch_install::dlx(ctx, args),
        CliCommand::Create(args) => dispatch_install::create(ctx, args),
        CliCommand::Runtime(args) => dispatch_install::runtime(ctx, args),
        CliCommand::Env(args) => dispatch_install::env(ctx, args),
        CliCommand::ApproveBuilds(args) => dispatch_install::approve_builds(ctx, args),
        CliCommand::Link(args) => dispatch_install::link(ctx, args),
        CliCommand::Import(args) => dispatch_install::import(ctx, args),
        CliCommand::Dedupe(args) => dispatch_install::dedupe(ctx, args),
        CliCommand::Deploy(args) => dispatch_install::deploy(ctx, args),
        CliCommand::Prune(args) => dispatch_install::prune(ctx, args),
        CliCommand::Fetch(args) => dispatch_install::fetch(ctx, args),
        CliCommand::Unlink(args) => dispatch_install::unlink(ctx, args),
        command => route_registry(command, ctx),
    }
}

fn route_registry<'a>(command: CliCommand, ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Access(args) => dispatch_query::access(ctx, args),
        CliCommand::Outdated(args) => dispatch_query::outdated(ctx, args),
        CliCommand::Audit(args) => dispatch_query::audit(ctx, args),
        CliCommand::Bugs(args) => dispatch_query::bugs(ctx, args),
        CliCommand::View(args) => dispatch_query::view(ctx, args),
        CliCommand::Whoami => dispatch_query::whoami(ctx),
        CliCommand::Star(args) => dispatch_query::star(ctx, args),
        CliCommand::Unstar(args) => dispatch_query::unstar(ctx, args),
        CliCommand::Stars(args) => dispatch_query::stars(ctx, args),
        CliCommand::DistTag(args) => dispatch_query::dist_tag(ctx, args),
        CliCommand::Team(args) => dispatch_query::team(ctx, args),
        CliCommand::Owner(args) => dispatch_query::owner(ctx, args),
        CliCommand::Deprecate(args) => dispatch_query::deprecate(ctx, args),
        CliCommand::Undeprecate(args) => dispatch_query::undeprecate(ctx, args),
        CliCommand::Unpublish(args) => dispatch_query::unpublish(ctx, args),
        CliCommand::Ping(args) => dispatch_query::ping(ctx, args),
        CliCommand::Search(args) => dispatch_query::search(ctx, args),
        CliCommand::Publish(args) => dispatch_query::publish(ctx, args),
        CliCommand::Token(_) => dispatch_query::not_implemented("token"),
        CliCommand::Docs(args) => dispatch_query::docs(ctx, args),
        CliCommand::Repo(args) => dispatch_query::repo(ctx, args),
        CliCommand::Login(args) => dispatch_query::login(ctx, args),
        CliCommand::Logout(args) => dispatch_query::logout(ctx, args),
        command => route_project(command, ctx),
    }
}

fn route_project<'a>(command: CliCommand, ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Init(args) => dispatch_script::init(ctx, &args),
        CliCommand::SetScript(args) => dispatch_script::set_script(ctx, args),
        CliCommand::Test(args) => dispatch_script::test(ctx, args),
        CliCommand::Run(args) => dispatch_script::run(ctx, args),
        CliCommand::External(command) => dispatch_script::fallback(ctx, command),
        CliCommand::Exec(args) => dispatch_script::exec(ctx, args),
        CliCommand::Start(args) => dispatch_script::start(ctx, args),
        CliCommand::Stop(args) => dispatch_script::stop(ctx, args),
        CliCommand::Restart(args) => dispatch_script::restart(ctx, args),
        CliCommand::Pkg(args) => dispatch_script::pkg(ctx, args),
        CliCommand::Edit(_) => dispatch_query::not_implemented("edit"),
        CliCommand::Profile(_) => dispatch_query::not_implemented("profile"),
        CliCommand::Xmas(_) => dispatch_query::not_implemented("xmas"),
        command => route_maintenance(command, ctx),
    }
}

fn route_maintenance<'a>(
    command: CliCommand,
    ctx: &RunCtx<'a>,
) -> miette::Result<CommandFuture<'a>> {
    match command {
        CliCommand::Recursive => dispatch_query::recursive(ctx),
        CliCommand::Change(args) => dispatch_query::change(ctx, args),
        CliCommand::Version(args) => dispatch_query::version(ctx, args),
        CliCommand::Lane(args) => dispatch_query::lane(ctx, args),
        CliCommand::List(args) => dispatch_query::list(ctx, args),
        CliCommand::Ll(args) => dispatch_query::ll(ctx, args),
        CliCommand::Licenses(args) => dispatch_query::licenses(ctx, args),
        CliCommand::Why(args) => dispatch_query::why(ctx, args),
        CliCommand::Sbom(args) => dispatch_query::sbom(ctx, args),
        CliCommand::Doctor(args) => dispatch_query::doctor(ctx, args),
        CliCommand::Pack(args) => dispatch_query::pack(ctx, args),
        CliCommand::Stage(args) => dispatch_query::stage(ctx, args),
        CliCommand::Peers(args) => dispatch_query::peers(ctx, args),
        CliCommand::FindHash(args) => dispatch_query::find_hash(ctx, args),
        CliCommand::Shim(args) => dispatch_query::shim(ctx, args),
        CliCommand::Bin(args) => dispatch_query::bin(ctx, args),
        CliCommand::Clean(args) => dispatch_query::clean(ctx, args, "clean"),
        CliCommand::Purge(args) => dispatch_query::clean(ctx, args, "purge"),
        CliCommand::Root(args) => dispatch_query::root(ctx, args),
        CliCommand::Prefix(args) => dispatch_query::prefix(ctx, args),
        CliCommand::Config(args) => dispatch_query::config(ctx, args),
        CliCommand::Get(args) => dispatch_query::config_get(ctx, args),
        CliCommand::Set(args) => dispatch_query::config_set(ctx, args),
        CliCommand::PackApp(args) => dispatch_query::pack_app(ctx, args),
        CliCommand::Store(command) => dispatch_query::store(ctx, command),
        CliCommand::Cache(command) => dispatch_query::cache(ctx, command),
        CliCommand::CatFile(args) => dispatch_query::cat_file(ctx, args),
        CliCommand::CatIndex(args) => dispatch_query::cat_index(ctx, args),
        CliCommand::IgnoredBuilds(args) => dispatch_query::ignored_builds(ctx, args),
        CliCommand::SelfUpdate(args) => dispatch_query::self_update(ctx, args),
        CliCommand::Setup(args) => dispatch_query::setup(ctx, args),
        CliCommand::With(args) => dispatch_query::with(ctx, args),
        CliCommand::Completion(_) | CliCommand::CompletionServer(_) => {
            unreachable!("completion returns before configuration")
        }
        _ => unreachable!("installation and registry commands are routed before project commands"),
    }
}

fn prints_json_errors(command: &CliCommand) -> bool {
    match command {
        CliCommand::Publish(args) => args.flags.json,
        CliCommand::View(args) => args.json,
        CliCommand::Pack(args) => args.json,
        _ => false,
    }
}

fn print_json_error(error: &miette::Report) {
    let code = error.code().map_or_else(|| "pnpm".to_string(), |code| code.to_string());
    let message = json_error_message(error);
    let mut error_body = serde_json::json!({
        "code": code,
        "message": message,
    });
    if let Some(otp_error) = otp_non_interactive_error(error) {
        if let Some(auth_url) = &otp_error.auth_url {
            error_body["authUrl"] = serde_json::Value::String(auth_url.clone());
        }
        if let Some(done_url) = &otp_error.done_url {
            error_body["doneUrl"] = serde_json::Value::String(done_url.clone());
        }
    }
    let output = serde_json::json!({
        "error": error_body,
    });
    // pnpm's `errorHandler` prints the envelope with `JSON.stringify(_, null, 2)`;
    // match its two-space indentation byte-for-byte.
    let output = serde_json::to_string_pretty(&output).expect("a JSON error envelope serializes");
    println!("{output}");
}

fn json_error_message(error: &miette::Report) -> String {
    let mut messages = error.chain().map(ToString::to_string);
    match (messages.next(), messages.next()) {
        (Some(context), Some(source)) if context == super::pack::PACK_ERROR_CONTEXT => source,
        (Some(message), _) => message,
        (None, _) => error.to_string(),
    }
}

fn otp_non_interactive_error(error: &miette::Report) -> Option<&OtpNonInteractiveError> {
    error
        .downcast_ref::<OtpNonInteractiveError>()
        .or_else(|| error.chain().find_map(|cause| cause.downcast_ref::<OtpNonInteractiveError>()))
}

fn now_millis() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

#[cfg(test)]
mod tests;

/// Install fast paths emit the same completion event as the full command.
fn emit_execution_time(emit: fn(&LogEvent), started_at: u128) {
    emit(&LogEvent::ExecutionTime(ExecutionTimeLog {
        level: LogLevel::Debug,
        started_at,
        ended_at: now_millis(),
    }));
}
