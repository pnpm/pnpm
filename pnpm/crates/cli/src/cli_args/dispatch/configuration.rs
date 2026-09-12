use super::{
    CliArgs, CliCommand, Config, Context, DefaultReporter, Host, IntoDiagnostic, NdjsonReporter,
    Path, PathBuf, ReporterType, SilentReporter, SummaryScope, apply_registry_override,
    apply_state_dir_override, apply_store_dir_override, configure_color, default_pnpm_home_dir,
    now_millis, prepare_config, prints_json_errors,
};

/// The directories a run is anchored at.
pub(super) struct RunAnchors {
    /// The canonicalized `--dir`.
    pub(super) dir: PathBuf,
    pub(super) cli_dir: PathBuf,
    pub(super) manifest_path: PathBuf,
    /// Where a `-g` install loads its config from. A `-g` install is
    /// isolated from the caller's project: pnpm runs it with `cwd` = the
    /// pnpm home dir, so a project `.npmrc` cannot influence the network /
    /// TLS / registry decisions of a *global* install. Mirror that by
    /// anchoring the global-install config at the pnpm home (resolved from
    /// `PNPM_HOME` / platform defaults, never the project). Falls back to
    /// the `--dir` anchor when the home can't be determined — the global
    /// command then fails at the missing-global-bin-dir check regardless.
    pub(super) global_config: PathBuf,
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
    pub(super) fn resolve(args: &CliArgs) -> miette::Result<Self> {
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
pub(super) struct RunSetup {
    pub(super) started_at: u128,
    pub(super) reporter: ReporterType,
    pub(super) is_install_family: bool,
    pub(super) print_json_errors: bool,
    pub(super) recursive_by_default: bool,
    pub(super) summary_scope: SummaryScope,
    pub(super) reports_scope: bool,
    pub(super) uses_stderr_reporter: bool,
}

impl RunSetup {
    pub(super) fn of(args: &CliArgs) -> Self {
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
pub(super) fn seed_config(npmrc_auth_file: Option<&Path>, ignore_workspace: bool) -> Config {
    Config {
        npmrc_auth_file: npmrc_auth_file.map(Path::to_path_buf),
        ignore_workspace,
        ..Config::default()
    }
}

/// `--color` wins over `--no-color`, and both over the configured value.
pub(super) fn apply_color_override(
    cfg: &mut Config,
    color: Option<pnpm_config::ColorMode>,
    no_color: bool,
) {
    cfg.color = match (color, no_color) {
        (Some(color), _) => color,
        (None, true) => pnpm_config::ColorMode::Never,
        (None, false) => cfg.color,
    };
    configure_color(cfg.color);
}

/// The CLI overrides that redirect where pnpm reads and writes.
pub(super) fn apply_location_overrides(
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
pub(super) struct ProjectSelectors<'a> {
    pub(super) recursive: bool,
    pub(super) recursive_by_default_command: bool,
    pub(super) filter: &'a [String],
    pub(super) filter_prod: &'a [String],
    pub(super) workspace_root: bool,
    pub(super) fail_if_no_match: bool,
}

pub(super) fn apply_project_selectors(cfg: &mut Config, selectors: &ProjectSelectors<'_>) {
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
pub(super) struct OutputOverrides<'a> {
    pub(super) reporter_hide_prefix: bool,
    pub(super) no_reporter_hide_prefix: bool,
    pub(super) workspace_packages: &'a [String],
    pub(super) test_pattern: &'a [String],
    pub(super) changed_files_ignore_pattern: &'a [String],
    pub(super) workspace_concurrency: Option<i32>,
}

pub(super) fn apply_output_overrides(cfg: &mut Config, overrides: &OutputOverrides<'_>) {
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

/// Install the project's config dependencies and apply their `updateConfig`
/// pnpmfile hooks to `config` before a command spawns anything, so a hook's
/// settings — `extraEnv` and `extraBinPaths` among them — reach the child
/// processes the command starts. pnpm applies them once per invocation,
/// whatever the command.
///
/// Every [`RunCtx::config`](super::RunCtx::config) call yields a fresh `Config`, so the pass has to
/// run on the instance the handler goes on to use — it cannot be hoisted ahead
/// of [`route`](super::routing::route). The install family and pack/publish apply the same pass at
/// their own entry points, where they already hold that instance.
pub(in super::super) async fn apply_update_config(
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
