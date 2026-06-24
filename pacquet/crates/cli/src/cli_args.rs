pub mod add;
pub mod approve_builds;
pub mod cache;
pub mod cat_file;
pub mod cat_index;
pub mod create;
pub mod dlx;
pub mod exec;
pub mod find_hash;
pub mod ignored_builds;
pub mod install;
pub mod outdated;
pub mod patch;
pub mod patch_commit;
pub mod patch_remove;
pub(crate) mod patch_state;
pub mod rebuild;
pub mod recursive;
pub mod remove;
pub mod restart;
pub mod run;
pub mod runtime;
pub mod sanitize;
pub mod stop;
pub mod store;
pub mod supported_architectures;
pub mod update;
pub mod update_interactive;
pub mod why;

use crate::{State, config_deps, config_overrides::ConfigOverrides};
use add::AddArgs;
use approve_builds::ApproveBuildsArgs;
use cache::CacheCommand;
use cat_file::CatFileArgs;
use cat_index::CatIndexArgs;
use clap::{Parser, Subcommand, ValueEnum};
use create::CreateArgs;
use dlx::DlxArgs;
use exec::ExecArgs;
use find_hash::FindHashArgs;
use ignored_builds::IgnoredBuildsArgs;
use install::InstallArgs;
use miette::{Context, IntoDiagnostic};
use outdated::{OutdatedArgs, OutdatedOutcome};
use pacquet_config::{Config, Host};
use pacquet_default_reporter::DefaultReporter;
use pacquet_executor::execute_shell;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::{
    ExecutionTimeLog, LogEvent, LogLevel, NdjsonReporter, Reporter, SilentReporter,
};
use patch::PatchArgs;
use patch_commit::PatchCommitArgs;
use patch_remove::PatchRemoveArgs;
use rebuild::RebuildArgs;
use remove::RemoveArgs;
use restart::RestartArgs;
use run::RunArgs;
use runtime::RuntimeArgs;
use serde_json::Value;
use std::{
    fs,
    future::Future,
    io::ErrorKind,
    path::{Path, PathBuf},
    pin::Pin,
};
use stop::StopArgs;
use store::StoreCommand;
use update::UpdateArgs;
use why::WhyArgs;

/// Experimental package manager for node.js written in rust.
#[derive(Debug, Parser)]
#[clap(name = "pacquet")]
#[clap(bin_name = "pacquet")]
#[clap(version = pacquet_config::PACQUET_VERSION)]
#[clap(about = "Experimental package manager for node.js")]
pub struct CliArgs {
    #[clap(subcommand)]
    pub command: CliCommand,

    /// Set working directory.
    #[clap(short = 'C', long, default_value = ".")]
    pub dir: PathBuf,

    /// Path to a `.npmrc` to read auth settings from, overriding the
    /// default `~/.npmrc`. Mirrors pnpm's `--npmrc-auth-file` (and its
    /// `--userconfig` alias) and sets
    /// [`pacquet_config::Config::npmrc_auth_file`], consumed when
    /// `Config` resolves the user-level `.npmrc`.
    #[clap(long = "npmrc-auth-file", visible_alias = "userconfig", global = true)]
    pub npmrc_auth_file: Option<PathBuf>,

    /// Run the command for every project in the workspace instead of
    /// only the project in `--dir`. Mirrors pnpm's global `-r` /
    /// `--recursive` flag and sets
    /// [`pacquet_config::Config::recursive`]. pacquet's `install`
    /// already spans the whole workspace, so the flag is a surface
    /// no-op there today; see the field docs.
    #[clap(short = 'r', long, global = true)]
    pub recursive: bool,

    /// Reporter output format.
    #[clap(long, value_enum, default_value_t = ReporterType::Default, global = true)]
    pub reporter: ReporterType,

    /// `--filter` / `-F` workspace selectors. Each occurrence adds one
    /// raw selector (`@scope/*`, `./pkg`, `foo...`, `!bar`, `{dir}`,
    /// `[since]`, ...). Stored into [`pacquet_config::Config::filter`];
    /// see that field for why the resolved selection is not yet
    /// consumed by `install`.
    ///
    /// As a global multi-value flag, occurrences collect only within one
    /// side of the subcommand boundary; mixing sides is a clap limitation,
    /// so pass all selectors on the same side.
    #[clap(short = 'F', long, global = true)]
    pub filter: Vec<String>,

    /// `--filter-prod` workspace selectors. Same syntax as
    /// [`Self::filter`], but the dependency walk follows production
    /// dependencies only. Stored into
    /// [`pacquet_config::Config::filter_prod`].
    #[clap(long = "filter-prod", global = true)]
    pub filter_prod: Vec<String>,
}

/// Selectable rendering strategy for log events.
///
/// Mirrors the names pnpm uses for `--reporter` (`default`, `append-only`,
/// `ndjson`, `silent`).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReporterType {
    /// pnpm-style visual output (progress line, packages diff, lifecycle
    /// output, `Done in ...`). The default; renders in place on a TTY and
    /// falls back to append-only when stdout is not a TTY.
    Default,
    /// Like `default` but forces the append-only rendering even on a TTY —
    /// one line per update, no cursor movement.
    AppendOnly,
    /// Newline-delimited JSON in pnpm's wire format on stderr.
    Ndjson,
    /// No progress output.
    Silent,
}

type CommandFuture<'a> = Pin<Box<dyn Future<Output = miette::Result<()>> + Send + 'a>>;

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Initialize a package.json
    Init,
    /// Add a package
    Add(AddArgs),
    /// Install packages
    Install(InstallArgs),
    /// Update packages to their newest version based on the specified range
    #[clap(visible_aliases = ["up", "upgrade"])]
    Update(UpdateArgs),
    /// Check for outdated packages
    Outdated(OutdatedArgs),
    /// Shows the packages that depend on `pkg`
    Why(WhyArgs),
    /// Rebuild a package.
    #[clap(visible_alias = "rb")]
    Rebuild(RebuildArgs),
    /// Removes packages from `node_modules` and from the project's `package.json`.
    // Unlike npm, pnpm does not treat "r" as an alias of "remove" to avoid
    // confusion with "run" and "recursive". Mirrors pnpm's `commandNames`.
    #[clap(visible_aliases = ["uninstall", "rm", "un", "uni"])]
    Remove(RemoveArgs),
    /// Prepare a package for patching.
    Patch(PatchArgs),
    /// Generate a patch out of a directory.
    #[clap(name = "patch-commit")]
    PatchCommit(PatchCommitArgs),
    /// Remove existing patch files.
    #[clap(name = "patch-remove")]
    PatchRemove(PatchRemoveArgs),
    /// Runs a package's "test" script, if one was provided.
    Test,
    /// Runs a defined package script.
    Run(RunArgs),
    /// Run a shell command in the context of a project.
    Exec(ExecArgs),
    /// Run a package in a temporary environment.
    Dlx(DlxArgs),
    /// Creates a project from a `create-*` starter kit.
    Create(CreateArgs),
    /// Runs an arbitrary command specified in the package's start property of its scripts object.
    Start,
    /// Runs a package's "stop" script, if one was provided.
    Stop(StopArgs),
    /// Restarts a package. Runs "stop", "restart", and "start" scripts,
    /// and associated pre- and post- scripts.
    Restart(RestartArgs),
    /// Lists the packages that include the file with the specified hash.
    FindHash(FindHashArgs),
    /// Manage runtimes.
    #[clap(visible_alias = "rt")]
    Runtime(RuntimeArgs),
    /// Managing the package store.
    #[clap(subcommand)]
    Store(StoreCommand),
    /// Inspect and manage the metadata cache.
    #[clap(subcommand)]
    Cache(CacheCommand),
    /// Prints the contents of a file based on the hash value stored in the index file.
    CatFile(CatFileArgs),
    /// Prints the index file of a specific package from the store.
    CatIndex(CatIndexArgs),
    /// Print the list of packages with blocked build scripts.
    IgnoredBuilds(IgnoredBuildsArgs),
    /// Approve dependencies for running scripts during installation.
    ApproveBuilds(ApproveBuildsArgs),
}

impl CliArgs {
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
        configure_default_reporter(self.reporter, &dir);
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
        let CliArgs { command, dir, npmrc_auth_file, recursive, reporter, filter, filter_prod } =
            self;
        // Canonicalize `--dir` so the bunyan-envelope `prefix` emitted by
        // the reporter is the same absolute, symlink-resolved path that
        // `@pnpm/cli.default-reporter` derives via `process.cwd()`. Without
        // this, a default `--dir=.` leaves `prefix` as `"."`, the reporter
        // never matches it against its `cwd`, and every progress / stats
        // line gets a redundant `.` path prefix prepended. Mirrors pnpm's
        // <https://github.com/pnpm/pnpm/blob/8eb1be4988/config/reader/src/index.ts#L270>
        // `cwd = fs.realpathSync(betterPathResolve(cliOptions.dir ...))`,
        // later assigned to `pnpmConfig.dir` (used as the install
        // `lockfileDir`, threaded into every event's `prefix`).
        let dir = dunce::canonicalize(&dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("canonicalizing the `--dir` argument: {}", dir.display()))?;
        // The default reporter renders paths relative to the install root and
        // its `Done in ...` footer over the whole command; seed both before any
        // event can fire.
        configure_default_reporter(reporter, &dir);
        let started_at = now_millis();
        let is_install_family = matches!(
            &command,
            CliCommand::Add(_)
                | CliCommand::Update(_)
                | CliCommand::Remove(_)
                | CliCommand::Install(_)
                | CliCommand::Dlx(_)
                | CliCommand::Create(_)
                | CliCommand::Runtime(_)
                // `rebuild` drives the frozen-install pipeline and emits
                // the same progress events, so it shares the `Done in ...`
                // footer.
                | CliCommand::Rebuild(_)
                | CliCommand::PatchCommit(_)
                | CliCommand::PatchRemove(_),
        );
        let manifest_path = dir.join("package.json");
        // Resolve `.npmrc` / `pnpm-workspace.yaml` from the canonicalized
        // `--dir` rather than the process cwd, matching pnpm 11 (which
        // builds its `localPrefix` from `cliOptions.dir`, not `cwd`) —
        // see [`loadNpmrcConfig`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/loadNpmrcFiles.ts#L48-L50).
        //
        // Production callers turbofish `Host` explicitly so the
        // dependency-injection plumbing is visible at the call site.
        // See [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339)
        // for the pattern and rationale.
        let config = || -> miette::Result<&'static mut Config> {
            // Seed `npmrc_auth_file` from the CLI flag before
            // `current()` reads `.npmrc`, so the override redirects the
            // user-level read. Mirrors pnpm's `--npmrc-auth-file`.
            Config { npmrc_auth_file: npmrc_auth_file.clone(), ..Config::default() }
                .current::<Host>(&dir)
                .map(|mut cfg| {
                    config_overrides.apply(&mut cfg);
                    // `--recursive` / `-r` is CLI-only upstream (not a
                    // `.npmrc` / yaml key), so it is set here from the
                    // global flag rather than through the yaml / env
                    // overlay. Mirrors pnpm's `Config.recursive`.
                    cfg.recursive = recursive;
                    // `--filter` / `--filter-prod` are likewise CLI-only
                    // (pnpm's `Config.filter` / `Config.filterProd`),
                    // so the parsed selector strings are threaded in
                    // from the global flags here.
                    cfg.filter.clone_from(&filter);
                    cfg.filter_prod.clone_from(&filter_prod);
                    Config::leak(cfg)
                })
                .map_err(miette::Report::new)
                .wrap_err("load configuration")
        };
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

        let dir_ref = &dir;
        let manifest_path_ref = &manifest_path;
        let command_future: CommandFuture<'_> = match command {
            CliCommand::Init => {
                let result =
                    PackageManifest::init(manifest_path_ref).wrap_err("initialize package.json");
                Box::pin(std::future::ready(result))
            }
            CliCommand::Add(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::Update(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            // `outdated` is a read-only query: it prints a report to
            // stdout and never installs, so it has no reporter-typed
            // install pipeline to dispatch on. It reports back whether any
            // dependency was outdated; process termination stays here, at
            // the top-level harness, rather than inside the command.
            CliCommand::Outdated(args) => {
                let command_state = state(false)?;
                Box::pin(async move {
                    if args.run(command_state).await? == OutdatedOutcome::Outdated {
                        std::process::exit(1);
                    }
                    Ok(())
                })
            }
            CliCommand::Why(args) => Box::pin(args.run(state(true)?)),
            CliCommand::Remove(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::Patch(args) => {
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                        args.run::<DefaultReporter>(dir_ref, command_state).await?;
                        Ok(())
                    }),
                    ReporterType::Ndjson => Box::pin(async move {
                        args.run::<NdjsonReporter>(dir_ref, command_state).await?;
                        Ok(())
                    }),
                    ReporterType::Silent => Box::pin(async move {
                        args.run::<SilentReporter>(dir_ref, command_state).await?;
                        Ok(())
                    }),
                }
            }
            CliCommand::PatchCommit(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                    if Box::pin(args.run::<DefaultReporter>(dir_ref, state(false)?)).await? {
                        Box::pin(
                            InstallArgs::for_patch_manifest_change()
                                .run::<DefaultReporter>(state(false)?),
                        )
                        .await?;
                    }
                    Ok(())
                }),
                ReporterType::Ndjson => Box::pin(async move {
                    if Box::pin(args.run::<NdjsonReporter>(dir_ref, state(false)?)).await? {
                        Box::pin(
                            InstallArgs::for_patch_manifest_change()
                                .run::<NdjsonReporter>(state(false)?),
                        )
                        .await?;
                    }
                    Ok(())
                }),
                ReporterType::Silent => Box::pin(async move {
                    if Box::pin(args.run::<SilentReporter>(dir_ref, state(false)?)).await? {
                        Box::pin(
                            InstallArgs::for_patch_manifest_change()
                                .run::<SilentReporter>(state(false)?),
                        )
                        .await?;
                    }
                    Ok(())
                }),
            },
            CliCommand::PatchRemove(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                    Box::pin(args.run(dir_ref, state(false)?)).await?;
                    Box::pin(
                        InstallArgs::for_patch_manifest_change()
                            .run::<DefaultReporter>(state(false)?),
                    )
                    .await?;
                    Ok(())
                }),
                ReporterType::Ndjson => Box::pin(async move {
                    Box::pin(args.run(dir_ref, state(false)?)).await?;
                    Box::pin(
                        InstallArgs::for_patch_manifest_change()
                            .run::<NdjsonReporter>(state(false)?),
                    )
                    .await?;
                    Ok(())
                }),
                ReporterType::Silent => Box::pin(async move {
                    Box::pin(args.run(dir_ref, state(false)?)).await?;
                    Box::pin(
                        InstallArgs::for_patch_manifest_change()
                            .run::<SilentReporter>(state(false)?),
                    )
                    .await?;
                    Ok(())
                }),
            },
            CliCommand::Install(args) => Box::pin(async move {
                // CLI overrides for `offline` / `prefer_offline` live
                // alongside `--frozen-lockfile`: they upgrade an
                // unset / `false` yaml value to `true`, but cannot
                // turn an explicit yaml `true` back off. Matches
                // pnpm's CLI semantics — the flags are "enable", not
                // a toggle. Applied here (between `config()` and
                // `State::init`) while the loaded `Config` is still
                // mutable through `Config::leak`'s
                // `&'static mut Config` return.
                let cfg = config()?;
                cfg.offline = cfg.offline || args.offline;
                cfg.prefer_offline = cfg.prefer_offline || args.prefer_offline;
                cfg.frozen_store = cfg.frozen_store || args.frozen_store;
                // `--ignore-scripts` enables (never toggles off) the
                // config value, matching the "enable" CLI flags above.
                cfg.ignore_scripts = cfg.ignore_scripts || args.ignore_scripts;
                cfg.workspace_concurrency =
                    args.resolve_workspace_concurrency(cfg.workspace_concurrency);
                // Network overrides: a passed `--network-concurrency` /
                // `--fetch-timeout` / `--user-agent` replaces the
                // config-resolved value for this invocation, matching
                // pnpm's "CLI wins" precedence.
                if let Some(network_concurrency) = args.network_concurrency {
                    cfg.network_concurrency = network_concurrency;
                }
                if let Some(fetch_timeout) = args.fetch_timeout {
                    cfg.fetch_timeout = fetch_timeout;
                }
                if let Some(user_agent) = args.user_agent.clone() {
                    cfg.user_agent = user_agent;
                }
                if let Some(pnpr_server) = args.pnpr_server.clone() {
                    cfg.pnpr_server = Some(pnpr_server);
                }
                let require_lockfile = args.frozen_lockfile;
                let frozen_lockfile = args.frozen_lockfile;
                // Config dependencies are workspace-level state: their
                // `.pnpm-config` and env lockfile live at the lockfile /
                // workspace root, not the CLI cwd. Use the same root
                // `State::init` uses (`config.workspace_dir`, set when a
                // `pnpm-workspace.yaml` is found), falling back to `--dir`
                // for a single-package repo. Owned so it doesn't hold a
                // borrow of `cfg` across the `&mut` `updateConfig` pass.
                let config_root = cfg.workspace_dir.clone().unwrap_or_else(|| dir_ref.clone());
                let package_manager_to_sync =
                    package_manager_to_sync(&config_root.join("package.json"), &config_root)
                        .wrap_err("read package manager policy")?;
                // Resolve + install configurational dependencies, then run
                // their `updateConfig` plugin hooks, before the main
                // install. The env lockfile must land at the top of
                // `pnpm-lock.yaml` before `State::init` loads the wanted
                // lockfile, and `updateConfig` must mutate `cfg` (still
                // `&'static mut`) before it's frozen and the install reads
                // it. Mirrors pnpm running both at config-finalization.
                let pipeline = InstallPipeline {
                    args,
                    cfg,
                    config_root,
                    package_manager_to_sync,
                    manifest_path: manifest_path_ref.clone(),
                    require_lockfile,
                    frozen_lockfile,
                };
                // Boxed for `clippy::large_stack_frames`: the three
                // monomorphized install futures would otherwise each reserve
                // their full size in this frame.
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(pipeline.run::<DefaultReporter>()).await?;
                    }
                    ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
                    ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
                }
                Ok(())
            }),
            CliCommand::Test => {
                let manifest = PackageManifest::from_path(manifest_path_ref.clone())
                    .wrap_err("getting the package.json in current directory")?;
                if let Some(script) = manifest.script("test", false)? {
                    execute_shell(script).wrap_err(format!("executing command: \"{script}\""))?;
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Run(args) => {
                if recursive {
                    args.run_recursive(config()?, dir_ref)?;
                } else {
                    args.run(dir_ref, config()?, matches!(reporter, ReporterType::Silent))?;
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Exec(args) => {
                if recursive {
                    args.run_recursive(config()?, dir_ref)?;
                } else {
                    args.run(dir_ref, config()?)?;
                }
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Dlx(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(dir_ref, config()?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir_ref, config()?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir_ref, config()?)),
            },
            CliCommand::Create(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(dir_ref, config()?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir_ref, config()?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir_ref, config()?)),
            },
            CliCommand::Start => {
                let manifest = PackageManifest::from_path(manifest_path_ref.clone())
                    .wrap_err("getting the package.json in current directory")?;
                let command = manifest.script("start", true)?.unwrap_or("node server.js");
                execute_shell(command).wrap_err(format!("executing command: \"{command}\""))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Stop(args) => {
                args.run(dir_ref, config()?, matches!(reporter, ReporterType::Silent))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Restart(args) => {
                args.run(dir_ref, config()?, matches!(reporter, ReporterType::Silent))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::FindHash(args) => {
                args.run(|| config().map(|m| &*m))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Runtime(args) => {
                args.reject_unsupported_global()?;
                let command_state = state(false)?;
                match reporter {
                    ReporterType::Default | ReporterType::AppendOnly => {
                        Box::pin(args.run::<DefaultReporter>(command_state))
                    }
                    ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
                    ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
                }
            }
            CliCommand::Store(command) => {
                command.run(|| config().map(|m| &*m))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Cache(command) => {
                command.run(config()?)?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::CatFile(args) => {
                args.run(|| config().map(|m| &*m))?;
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::CatIndex(args) => Box::pin(async move {
                args.run(dir_ref, || config().map(|m| &*m)).await?;
                Ok(())
            }),
            CliCommand::IgnoredBuilds(_) => {
                let output = ignored_builds::render_ignored_builds(config()?)?;
                print!("{output}");
                Box::pin(std::future::ready(Ok(())))
            }
            CliCommand::Rebuild(args) => match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(args.run::<DefaultReporter>(state(true)?))
                }
                ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(state(true)?)),
                ReporterType::Silent => Box::pin(args.run::<SilentReporter>(state(true)?)),
            },
            CliCommand::ApproveBuilds(args) => {
                // The settings/prompt work is synchronous; only the rebuild
                // is async, so the non-`Send` `config` / `state` closures
                // stay out of the awaited future.
                if let Some((rebuild_state, build_packages)) =
                    args.prepare(dir_ref, &config, &state)?
                {
                    let selected = Some(build_packages);
                    match reporter {
                        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
                            rebuild::run_rebuild::<DefaultReporter>(&rebuild_state, selected).await
                        }),
                        ReporterType::Ndjson => Box::pin(async move {
                            rebuild::run_rebuild::<NdjsonReporter>(&rebuild_state, selected).await
                        }),
                        ReporterType::Silent => Box::pin(async move {
                            rebuild::run_rebuild::<SilentReporter>(&rebuild_state, selected).await
                        }),
                    }
                } else {
                    Box::pin(std::future::ready(Ok(())))
                }
            }
        };

        command_future.await?;

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

/// The reporter-generic body of `pacquet install`: it threads one `Reporter`
/// type through config-dependency sync, the `updateConfig` hooks, and the
/// install itself. Lifting it out of the dispatch keeps the three
/// `ReporterType` arms to a single line each.
struct InstallPipeline {
    args: InstallArgs,
    cfg: &'static mut Config,
    config_root: PathBuf,
    package_manager_to_sync: Option<PackageManagerToSync>,
    manifest_path: PathBuf,
    require_lockfile: bool,
    frozen_lockfile: bool,
}

impl InstallPipeline {
    async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let InstallPipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            manifest_path,
            require_lockfile,
            frozen_lockfile,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                frozen_lockfile,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, frozen_lockfile).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let cfg: &'static Config = cfg;
        let state =
            State::init(manifest_path, cfg, require_lockfile).wrap_err("initialize the state")?;
        args.run::<Reporter>(state).await
    }
}

/// Resolve a [`ReporterType`] to the monomorphized `emit` of its sink, for
/// the event-emission sites that aren't already generic over `Reporter`.
fn reporter_emit(reporter: ReporterType) -> fn(&LogEvent) {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => DefaultReporter::emit,
        ReporterType::Ndjson => NdjsonReporter::emit,
        ReporterType::Silent => SilentReporter::emit,
    }
}

/// Seed the process-global default-reporter state that can't be recovered
/// from events: the project root (for relative paths) and, for
/// `--reporter=append-only`, the forced append-only mode. Idempotent; safe to
/// call from both the fast path and the main run.
fn configure_default_reporter(reporter: ReporterType, dir: &Path) {
    pacquet_default_reporter::set_cwd(dir.to_string_lossy().into_owned());
    if matches!(reporter, ReporterType::AppendOnly) {
        pacquet_default_reporter::force_append_only();
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

#[derive(Debug)]
struct WantedPackageManager {
    name: String,
    version: Option<String>,
    from_dev_engines: bool,
    on_fail: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct PackageManagerToSync {
    specifier: String,
    version: String,
}

fn package_manager_to_sync(
    manifest_path: &Path,
    root_dir: &Path,
) -> miette::Result<Option<PackageManagerToSync>> {
    let Some(manifest) = read_manifest_json(manifest_path)? else {
        return Ok(None);
    };
    let Some(pm) = wanted_package_manager(&manifest) else {
        return Ok(None);
    };
    let Some(wanted_version) = pm.version.as_deref() else {
        return Ok(None);
    };
    if pm.name != "pnpm" || !should_persist_package_manager_lockfile(&pm) {
        return Ok(None);
    }
    let source_version = current_source_pnpm_version().or_else(|| pnpm_version_from(root_dir));
    if let Some(version) =
        source_version.filter(|version| version_satisfies(version, wanted_version))
    {
        return Ok(Some(PackageManagerToSync { specifier: wanted_version.to_string(), version }));
    }
    Ok(exact_version(wanted_version)
        .filter(|version| version_satisfies(version, wanted_version))
        .map(|version| PackageManagerToSync { specifier: wanted_version.to_string(), version }))
}

fn read_manifest_json(path: &Path) -> miette::Result<Option<Value>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_diagnostic(),
    };
    serde_json::from_str(&content).into_diagnostic().map(Some)
}

fn wanted_package_manager(manifest: &Value) -> Option<WantedPackageManager> {
    if let Some(mut pm) = parse_dev_engines_package_manager(manifest) {
        if pm.version.as_deref().is_some_and(|version| node_semver::Range::parse(version).is_err())
        {
            pm.version = None;
        }
        return Some(pm);
    }
    let package_manager = manifest.get("packageManager")?.as_str()?;
    let (name, version) = parse_package_manager(package_manager);
    let version = version.and_then(|version| exact_version(&version));
    Some(WantedPackageManager { name, version, from_dev_engines: false, on_fail: None })
}

fn parse_dev_engines_package_manager(manifest: &Value) -> Option<WantedPackageManager> {
    let value = manifest.get("devEngines")?.get("packageManager")?;
    if let Some(items) = value.as_array() {
        if items.is_empty() {
            return None;
        }
        let index = items
            .iter()
            .position(|item| item.get("name").and_then(Value::as_str) == Some("pnpm"))
            .unwrap_or(0);
        let item = &items[index];
        let on_fail =
            item.get("onFail").and_then(Value::as_str).map(ToString::to_string).or_else(|| {
                Some(if index == items.len() - 1 { "error" } else { "ignore" }.to_string())
            });
        return package_manager_from_engine(item, true, on_fail);
    }
    package_manager_from_engine(
        value,
        true,
        value.get("onFail").and_then(Value::as_str).map(ToString::to_string),
    )
}

fn package_manager_from_engine(
    value: &Value,
    from_dev_engines: bool,
    on_fail: Option<String>,
) -> Option<WantedPackageManager> {
    Some(WantedPackageManager {
        name: value.get("name")?.as_str()?.to_string(),
        version: value.get("version").and_then(Value::as_str).map(ToString::to_string),
        from_dev_engines,
        on_fail,
    })
}

fn parse_package_manager(package_manager: &str) -> (String, Option<String>) {
    let Some((name, reference)) = package_manager.split_once('@') else {
        return (package_manager.to_string(), None);
    };
    if reference.contains(':') {
        return (name.to_string(), None);
    }
    (
        name.to_string(),
        Some(reference.split_once('+').map_or(reference, |(version, _)| version).to_string()),
    )
}

fn should_persist_package_manager_lockfile(pm: &WantedPackageManager) -> bool {
    if pm.on_fail.as_deref().unwrap_or("download") == "ignore" {
        return false;
    }
    if pm.from_dev_engines {
        return true;
    }
    pm.version
        .as_deref()
        .and_then(|version| node_semver::Version::parse(version).ok())
        .is_some_and(|version| version.major >= 12)
}

fn current_source_pnpm_version() -> Option<String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.ancestors().find_map(pnpm_version_from)
}

fn pnpm_version_from(root_dir: &Path) -> Option<String> {
    let path = root_dir.join("pnpm11").join("pnpm").join("package.json");
    let value = read_manifest_json(&path).ok()??;
    value.get("version").and_then(Value::as_str).map(ToString::to_string)
}

fn exact_version(version: &str) -> Option<String> {
    let parsed = node_semver::Version::parse(version).ok()?;
    (parsed.to_string() == version).then(|| version.to_string())
}

fn version_satisfies(version: &str, wanted_range: &str) -> bool {
    let Ok(version) = node_semver::Version::parse(version) else {
        return false;
    };
    let Ok(range) = node_semver::Range::parse(wanted_range) else {
        return false;
    };
    if version.satisfies(&range) {
        return true;
    }
    if version.pre_release.is_empty() {
        return false;
    }
    let base = node_semver::Version {
        major: version.major,
        minor: version.minor,
        patch: version.patch,
        pre_release: Vec::new(),
        build: version.build,
    };
    base.satisfies(&range)
}

#[cfg(test)]
mod tests;
