use super::{
    cli_command::{CliArgs, CliCommand},
    dispatch_install, dispatch_query, dispatch_script,
    reporter::{ReporterType, configure_default_reporter, reporter_emit},
};
use crate::{
    State,
    config_overrides::{ConfigOverrides, apply_store_dir_override},
};
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Config, Host, default_pnpm_home_dir};
use pacquet_default_reporter::SummaryScope;
use pacquet_network_web_auth::OtpNonInteractiveError;
use pacquet_reporter::{ExecutionTimeLog, LogEvent, LogLevel};
use std::{future::Future, path::Path, pin::Pin};

pub(crate) type CommandFuture<'a> = Pin<Box<dyn Future<Output = miette::Result<()>> + Send + 'a>>;

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
    pub(crate) manifest_path: &'a Path,
    pub(crate) reporter: ReporterType,
    pub(crate) recursive: bool,
    pub(crate) recursive_resume_from: Option<&'a str>,
    pub(crate) recursive_report_summary: bool,
    pub(crate) recursive_no_bail: bool,
    pub(crate) recursive_sort: bool,
    /// The top-level `--if-present` spelling (`pnpm --if-present test`);
    /// merged with the flag the script subcommands declare themselves.
    pub(crate) if_present: bool,
    pub(crate) config: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    /// Like [`Self::config`] but anchored at the pnpm home dir instead of
    /// `--dir`, so a `-g` install can't inherit the caller project's
    /// `.npmrc` network / TLS / registry settings.
    pub(crate) global_config: &'a (dyn Fn() -> miette::Result<&'static mut Config> + Sync),
    pub(crate) state: &'a (dyn Fn(bool) -> miette::Result<State> + Sync),
}

impl CliArgs {
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
    /// file seed + `--config.<key>` overrides). Workspace-filtered and
    /// recursive installs always take the full path.
    pub fn finished_via_install_fast_path(&self, config_overrides: &ConfigOverrides) -> bool {
        let started_at = now_millis();
        let CliCommand::Install(install_args) = &self.command else {
            return false;
        };
        if self.recursive || !self.filter.is_empty() || !self.filter_prod.is_empty() {
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
        config_overrides.apply(&mut config);
        if let Some(store_dir) = self.store_dir.as_deref()
            && apply_store_dir_override::<Host>(&mut config, store_dir, &dir).is_err()
        {
            return false;
        }
        configure_default_reporter(self.reporter, &dir, SummaryScope::CurrentPrefix);
        let emit = reporter_emit(self.reporter);
        let finished = install_args.finished_via_up_to_date_fast_path(&dir, &config, emit);
        if finished {
            // The fast path returns from `main` before `run` reaches its
            // end-of-command emit, so the `Done in ...` footer must be emitted
            // here too to match the non-fast-path output.
            emit(&LogEvent::ExecutionTime(ExecutionTimeLog {
                level: LogLevel::Debug,
                started_at,
                ended_at: now_millis(),
            }));
        }
        finished
    }

    /// Execute the command. `config_overrides` carries `--config.<key>=<value>`
    /// tokens already stripped from argv by [`ConfigOverrides::extract`];
    /// they're layered on top of `.npmrc` / `pnpm-workspace.yaml` whenever
    /// `Config` is loaded, mirroring pnpm 11's
    /// "CLI > yaml > .npmrc > defaults" precedence.
    pub async fn run(self, config_overrides: &ConfigOverrides) -> miette::Result<()> {
        if self.run_completion_if_requested()? {
            return Ok(());
        }

        // `version` short-circuits in `main`, never reaching dispatch.
        let CliArgs {
            command,
            dir,
            store_dir,
            npmrc_auth_file,
            recursive,
            reporter,
            filter,
            filter_prod,
            test_pattern,
            changed_files_ignore_pattern,
            version: _,
            color: _,
            yes: _,
            sort: _,
            no_sort,
            workspace_concurrency,
            resume_from,
            report_summary,
            no_bail,
            if_present,
        } = self;

        // Canonicalize `--dir` so the bunyan-envelope `prefix` emitted by
        // the reporter is the same absolute, symlink-resolved path that
        // `@pnpm/cli.default-reporter` derives via `process.cwd()`. Without
        // this, a default `--dir=.` leaves `prefix` as `"."`, the reporter
        // never matches it against its `cwd`, and every progress / stats
        // line gets a redundant `.` path prefix prepended. The resolved
        // path becomes `config.dir` (used as the install `lockfileDir`,
        // threaded into every event's `prefix`).
        let dir = dunce::canonicalize(&dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("canonicalizing the `--dir` argument: {}", dir.display()))?;
        // The default reporter renders paths relative to the install root and
        // its `Done in ...` footer over the whole command; seed both before any
        // event can fire.
        configure_default_reporter(reporter, &dir, command.default_reporter_summary_scope());
        let started_at = now_millis();
        let is_install_family = matches!(
            &command,
            CliCommand::Add(_)
                | CliCommand::Update(_)
                | CliCommand::Remove(_)
                | CliCommand::Install(_)
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
        );
        let print_json_errors = prints_json_errors(&command);
        let manifest_path = dir.join("package.json");
        // Load config anchored at `anchor`, reading `.npmrc` /
        // `pnpm-workspace.yaml` from there.
        //
        // Seed `npmrc_auth_file` from the CLI flag before `current()` reads
        // `.npmrc`, so the override redirects the user-level read. Mirrors
        // pnpm's `--npmrc-auth-file`. Production callers turbofish `Host`
        // explicitly so the dependency-injection plumbing is visible at the
        // call site. See
        // [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339).
        let load_config = |anchor: &Path| -> miette::Result<&'static mut Config> {
            Config { npmrc_auth_file: npmrc_auth_file.clone(), ..Config::default() }
                .current::<Host>(anchor)
                .map_err(miette::Report::new)
                .wrap_err("load configuration")
                .and_then(|mut cfg| {
                    config_overrides.apply(&mut cfg);
                    if let Some(store_dir) = store_dir.as_deref() {
                        apply_store_dir_override::<Host>(&mut cfg, store_dir, anchor)?;
                    }
                    // `--recursive` / `--filter` / `--filter-prod` are
                    // CLI-only upstream (not `.npmrc` / yaml keys), so the
                    // global flags are threaded in here. Mirrors pnpm's
                    // `Config.recursive` / `.filter` / `.filterProd`.
                    cfg.recursive = recursive;
                    cfg.filter.clone_from(&filter);
                    cfg.filter_prod.clone_from(&filter_prod);
                    // Unlike the CLI-only selectors above, these two are
                    // genuine config keys — the flag overrides yaml / env
                    // only when actually given.
                    if !test_pattern.is_empty() {
                        cfg.test_pattern.clone_from(&test_pattern);
                    }
                    if !changed_files_ignore_pattern.is_empty() {
                        cfg.changed_files_ignore_pattern.clone_from(&changed_files_ignore_pattern);
                    }
                    if let Some(workspace_concurrency) = workspace_concurrency {
                        cfg.workspace_concurrency =
                            pacquet_config::resolve_child_concurrency(Some(workspace_concurrency));
                    }
                    Ok(Config::leak(cfg))
                })
        };
        // Resolve `.npmrc` / `pnpm-workspace.yaml` from the canonicalized
        // `--dir` rather than the process cwd, matching pnpm 11 (which
        // builds its `localPrefix` from `cliOptions.dir`, not `cwd`).
        let config = || load_config(&dir);
        // A `-g` install is isolated from the caller's project: pnpm runs it
        // with `cwd` = the pnpm home dir, so a project `.npmrc` cannot
        // influence the network / TLS / registry decisions of a *global*
        // install. Mirror that by anchoring the global-install config at the
        // pnpm home (resolved from `PNPM_HOME` / platform defaults, never the
        // project). Falls back to the `--dir` anchor when the home can't be
        // determined — the global command then fails at the missing-global-
        // bin-dir check regardless.
        let pnpm_home_dir = default_pnpm_home_dir::<Host>();
        let global_config_anchor = pnpm_home_dir.as_deref().unwrap_or(&dir);
        let global_config = || load_config(global_config_anchor);
        // `require_lockfile` is the "this subcommand cannot run without a
        // lockfile loaded" signal, used by `State::init` to override
        // `config.lockfile=false`. Only `install --frozen-lockfile` needs
        // it today; other subcommands follow `config.lockfile`. Matches
        // pnpm's CLI: `--frozen-lockfile` is the strongest signal and
        // must not be silently dropped because `lockfile=false` was set
        // (or defaulted) in config.
        let state = |require_lockfile: bool| -> miette::Result<State> {
            State::init(manifest_path.clone(), config()?, require_lockfile)
                .wrap_err("initialize the state")
        };

        let ctx = RunCtx {
            dir: &dir,
            manifest_path: &manifest_path,
            reporter,
            recursive,
            recursive_resume_from: resume_from.as_deref(),
            recursive_report_summary: report_summary,
            recursive_no_bail: no_bail,
            recursive_sort: !no_sort,
            if_present,
            config: &config,
            global_config: &global_config,
            state: &state,
        };
        match route(command, &ctx) {
            Ok(future) => {
                if let Err(error) = future.await {
                    if print_json_errors {
                        print_json_error(&error);
                        std::process::exit(1);
                    }
                    return Err(error);
                }
            }
            Err(error) => {
                if print_json_errors {
                    print_json_error(&error);
                    std::process::exit(1);
                }
                return Err(error);
            }
        }

        // The `Done in ...` footer covers the whole command, mirroring pnpm's
        // `pnpm:execution-time` emit in `main.ts`. Only the install-family
        // commands drive the visual reporter, so the rest stay silent.
        if is_install_family {
            reporter_emit(reporter)(&LogEvent::ExecutionTime(ExecutionTimeLog {
                level: LogLevel::Debug,
                started_at,
                ended_at: now_millis(),
            }));
        }

        Ok(())
    }
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
        CliCommand::Access(args) => dispatch_query::access(ctx, args),
        CliCommand::Init => dispatch_script::init(ctx),
        CliCommand::Add(args) => dispatch_install::add(ctx, args),
        CliCommand::Install(args) => dispatch_install::install(ctx, args),
        CliCommand::Update(args) => dispatch_install::update(ctx, args),
        CliCommand::Outdated(args) => dispatch_query::outdated(ctx, args),
        CliCommand::Audit(args) => dispatch_query::audit(ctx, args),
        CliCommand::Change(args) => dispatch_query::change(ctx, args),
        CliCommand::Version(args) => dispatch_query::version(ctx, args),
        CliCommand::Lane(args) => dispatch_query::lane(ctx, args),
        CliCommand::Bugs(args) => dispatch_query::bugs(ctx, args),
        CliCommand::List(args) => dispatch_query::list(ctx, args),
        CliCommand::Ll(args) => dispatch_query::ll(ctx, args),
        CliCommand::Why(args) => dispatch_query::why(ctx, args),
        CliCommand::Sbom(args) => dispatch_query::sbom(ctx, args),
        CliCommand::Whoami => dispatch_query::whoami(ctx),
        CliCommand::Star(args) => dispatch_query::star(ctx, args),
        CliCommand::Unstar(args) => dispatch_query::unstar(ctx, args),
        CliCommand::Stars(args) => dispatch_query::stars(ctx, args),
        CliCommand::DistTag(args) => dispatch_query::dist_tag(ctx, args),
        CliCommand::Team(args) => dispatch_query::team(ctx, args),
        CliCommand::Owner(args) => dispatch_query::owner(ctx, args),
        CliCommand::Deprecate(args) => dispatch_query::deprecate(ctx, args),
        CliCommand::Undeprecate(args) => dispatch_query::undeprecate(ctx, args),
        CliCommand::Ping(args) => dispatch_query::ping(ctx, args),
        CliCommand::Search(args) => dispatch_query::search(ctx, args),
        CliCommand::Rebuild(args) => dispatch_install::rebuild(ctx, args),
        CliCommand::Pack(args) => dispatch_query::pack(ctx, args),
        CliCommand::Publish(args) => dispatch_query::publish(ctx, args),
        CliCommand::Stage(args) => dispatch_query::stage(ctx, args),
        CliCommand::Remove(args) => dispatch_install::remove(ctx, args),
        CliCommand::Patch(args) => dispatch_install::patch(ctx, args),
        CliCommand::PatchCommit(args) => dispatch_install::patch_commit(ctx, args),
        CliCommand::PatchRemove(args) => dispatch_install::patch_remove(ctx, args),
        CliCommand::Peers(args) => dispatch_query::peers(ctx, args),
        CliCommand::SetScript(args) => dispatch_script::set_script(ctx, args),
        CliCommand::Test => dispatch_script::test(ctx),
        CliCommand::Run(args) => dispatch_script::run(ctx, args),
        CliCommand::External(command) => dispatch_script::fallback(ctx, command),
        CliCommand::Exec(args) => dispatch_script::exec(ctx, args),
        CliCommand::Dlx(args) => dispatch_install::dlx(ctx, args),
        CliCommand::Create(args) => dispatch_install::create(ctx, args),
        CliCommand::Start => dispatch_script::start(ctx),
        CliCommand::Stop(args) => dispatch_script::stop(ctx, args),
        CliCommand::Restart(args) => dispatch_script::restart(ctx, args),
        CliCommand::FindHash(args) => dispatch_query::find_hash(ctx, args),
        CliCommand::Runtime(args) => dispatch_install::runtime(ctx, args),
        CliCommand::Bin(args) => dispatch_query::bin(ctx, args),
        CliCommand::Clean(args) => dispatch_query::clean(ctx, args, "clean"),
        CliCommand::Purge(args) => dispatch_query::clean(ctx, args, "purge"),
        CliCommand::Root(args) => dispatch_query::root(ctx, args),
        CliCommand::Prefix(args) => dispatch_query::prefix(ctx, args),
        CliCommand::Config(args) => dispatch_query::config(ctx, args),
        CliCommand::Pkg(args) => dispatch_script::pkg(ctx, args),
        CliCommand::PackApp(args) => dispatch_query::pack_app(ctx, args),
        CliCommand::Store(command) => dispatch_query::store(ctx, command),
        CliCommand::Cache(command) => dispatch_query::cache(ctx, command),
        CliCommand::CatFile(args) => dispatch_query::cat_file(ctx, args),
        CliCommand::CatIndex(args) => dispatch_query::cat_index(ctx, args),
        CliCommand::IgnoredBuilds(args) => dispatch_query::ignored_builds(ctx, args),
        CliCommand::ApproveBuilds(args) => dispatch_install::approve_builds(ctx, args),
        CliCommand::Link(args) => dispatch_install::link(ctx, args),
        CliCommand::Import(args) => dispatch_install::import(ctx, args),
        CliCommand::Dedupe(args) => dispatch_install::dedupe(ctx, args),
        CliCommand::Deploy(args) => dispatch_install::deploy(ctx, args),
        CliCommand::Prune(args) => dispatch_install::prune(ctx, args),
        CliCommand::Fetch(args) => dispatch_install::fetch(ctx, args),
        CliCommand::Unlink(args) => dispatch_install::unlink(ctx, args),
        CliCommand::Docs(args) => dispatch_query::docs(ctx, args),
        CliCommand::Repo(args) => dispatch_query::repo(ctx, args),
        CliCommand::SelfUpdate(args) => dispatch_query::self_update(ctx, args),
        CliCommand::Setup(args) => dispatch_query::setup(ctx, args),
        CliCommand::Login(args) => dispatch_query::login(ctx, args),
        CliCommand::Logout(args) => dispatch_query::logout(ctx, args),
        CliCommand::With(args) => dispatch_query::with(ctx, args),
        CliCommand::Completion(_) | CliCommand::CompletionServer(_) => {
            unreachable!("completion returns before configuration")
        }
    }
}

fn prints_json_errors(command: &CliCommand) -> bool {
    matches!(command, CliCommand::Publish(args) if args.flags.json)
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
