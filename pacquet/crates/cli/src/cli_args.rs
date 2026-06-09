pub mod add;
pub mod dlx;
pub mod exec;
pub mod install;
pub mod outdated;
pub mod recursive;
pub mod remove;
pub mod run;
pub mod store;
pub mod supported_architectures;
pub mod update;
pub mod update_interactive;

use crate::{State, config_deps, config_overrides::ConfigOverrides};
use add::AddArgs;
use clap::{Parser, Subcommand, ValueEnum};
use dlx::DlxArgs;
use exec::ExecArgs;
use install::InstallArgs;
use miette::{Context, IntoDiagnostic};
use outdated::{OutdatedArgs, OutdatedOutcome};
use pacquet_config::{Config, Host};
use pacquet_executor::execute_shell;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::{NdjsonReporter, SilentReporter};
use remove::RemoveArgs;
use run::RunArgs;
use std::path::PathBuf;
use store::StoreCommand;
use update::UpdateArgs;

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
    #[clap(long, value_enum, default_value_t = ReporterType::Silent, global = true)]
    pub reporter: ReporterType,

    /// `--filter` / `-F` workspace selectors. Each occurrence adds one
    /// raw selector (`@scope/*`, `./pkg`, `foo...`, `!bar`, `{dir}`,
    /// `[since]`, ...). Stored into [`pacquet_config::Config::filter`];
    /// see that field for why the resolved selection is not yet
    /// consumed by `install`.
    ///
    /// As a global multi-value flag, occurrences collect only within one
    /// side of the subcommand boundary: `pacquet -F a -F b install` and
    /// `pacquet install -F a -F b` both yield `[a, b]`, but mixing sides
    /// (`pacquet -F a install -F b`) keeps only the subcommand-side
    /// occurrence. This is a clap limitation; pass all selectors on the
    /// same side.
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
/// Mirrors the names pnpm uses for `--reporter` (`default`, `ndjson`,
/// `silent`, `append-only`). Only the variants pacquet currently supports
/// are listed; the others land alongside the default-reporter spawn-and-
/// pipe (tracked under [#344]).
///
/// [#344]: https://github.com/pnpm/pacquet/issues/344
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReporterType {
    /// Newline-delimited JSON in pnpm's wire format on stderr.
    Ndjson,
    /// No progress output.
    Silent,
}

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
    /// Removes packages from `node_modules` and from the project's `package.json`.
    // Unlike npm, pnpm does not treat "r" as an alias of "remove" to avoid
    // confusion with "run" and "recursive". Mirrors pnpm's `commandNames`.
    #[clap(visible_aliases = ["uninstall", "rm", "un", "uni"])]
    Remove(RemoveArgs),
    /// Runs a package's "test" script, if one was provided.
    Test,
    /// Runs a defined package script.
    Run(RunArgs),
    /// Run a shell command in the context of a project.
    Exec(ExecArgs),
    /// Run a package in a temporary environment.
    Dlx(DlxArgs),
    /// Runs an arbitrary command specified in the package's start property of its scripts object.
    Start,
    /// Managing the package store.
    #[clap(subcommand)]
    Store(StoreCommand),
}

impl CliArgs {
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
        let manifest_path = || dir.join("package.json");
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
            State::init(manifest_path(), config()?, require_lockfile)
                .wrap_err("initialize the state")
        };

        match command {
            CliCommand::Init => {
                PackageManifest::init(&manifest_path()).wrap_err("initialize package.json")?;
            }
            CliCommand::Add(args) => match reporter {
                ReporterType::Ndjson => args.run::<NdjsonReporter>(state(false)?).await?,
                ReporterType::Silent => args.run::<SilentReporter>(state(false)?).await?,
            },
            CliCommand::Update(args) => match reporter {
                ReporterType::Ndjson => args.run::<NdjsonReporter>(state(false)?).await?,
                ReporterType::Silent => args.run::<SilentReporter>(state(false)?).await?,
            },
            // `outdated` is a read-only query: it prints a report to
            // stdout and never installs, so it has no reporter-typed
            // install pipeline to dispatch on. It reports back whether any
            // dependency was outdated; process termination stays here, at
            // the top-level harness, rather than inside the command.
            CliCommand::Outdated(args) => {
                if args.run(state(false)?).await? == OutdatedOutcome::Outdated {
                    std::process::exit(1);
                }
            }
            CliCommand::Remove(args) => match reporter {
                ReporterType::Ndjson => args.run::<NdjsonReporter>(state(false)?).await?,
                ReporterType::Silent => args.run::<SilentReporter>(state(false)?).await?,
            },
            CliCommand::Install(args) => {
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
                let config_root = cfg.workspace_dir.clone().unwrap_or_else(|| dir.clone());
                // Resolve + install configurational dependencies, then run
                // their `updateConfig` plugin hooks, before the main
                // install. The env lockfile must land at the top of
                // `pnpm-lock.yaml` before `State::init` loads the wanted
                // lockfile, and `updateConfig` must mutate `cfg` (still
                // `&'static mut`) before it's frozen and the install reads
                // it. Mirrors pnpm running both at config-finalization.
                match reporter {
                    ReporterType::Ndjson => {
                        config_deps::install_config_deps::<NdjsonReporter>(
                            cfg,
                            &config_root,
                            frozen_lockfile,
                        )
                        .await?;
                        config_deps::run_update_config_hooks::<NdjsonReporter>(cfg, &config_root)
                            .await?;
                        let cfg: &'static Config = cfg;
                        let state = State::init(manifest_path(), cfg, require_lockfile)
                            .wrap_err("initialize the state")?;
                        args.run::<NdjsonReporter>(state).await?;
                    }
                    ReporterType::Silent => {
                        config_deps::install_config_deps::<SilentReporter>(
                            cfg,
                            &config_root,
                            frozen_lockfile,
                        )
                        .await?;
                        config_deps::run_update_config_hooks::<SilentReporter>(cfg, &config_root)
                            .await?;
                        let cfg: &'static Config = cfg;
                        let state = State::init(manifest_path(), cfg, require_lockfile)
                            .wrap_err("initialize the state")?;
                        args.run::<SilentReporter>(state).await?;
                    }
                }
            }
            CliCommand::Test => {
                let manifest = PackageManifest::from_path(manifest_path())
                    .wrap_err("getting the package.json in current directory")?;
                if let Some(script) = manifest.script("test", false)? {
                    execute_shell(script).wrap_err(format!("executing command: \"{script}\""))?;
                }
            }
            CliCommand::Run(args) => {
                if recursive {
                    args.run_recursive(config()?, &dir)?;
                } else {
                    args.run(&dir, config()?, matches!(reporter, ReporterType::Silent))?;
                }
            }
            CliCommand::Exec(args) => {
                if recursive {
                    args.run_recursive(config()?, &dir)?;
                } else {
                    args.run(&dir, config()?)?;
                }
            }
            CliCommand::Dlx(args) => match reporter {
                ReporterType::Ndjson => args.run::<NdjsonReporter>(&dir, config()?).await?,
                ReporterType::Silent => args.run::<SilentReporter>(&dir, config()?).await?,
            },
            CliCommand::Start => {
                // Runs an arbitrary command specified in the package's start property of its scripts
                // object. If no start property is specified on the scripts object, it will attempt to
                // run node server.js as a default, failing if neither are present.
                // The intended usage of the property is to specify a command that starts your program.
                let manifest = PackageManifest::from_path(manifest_path())
                    .wrap_err("getting the package.json in current directory")?;
                let command = manifest.script("start", true)?.unwrap_or("node server.js");
                execute_shell(command).wrap_err(format!("executing command: \"{command}\""))?;
            }
            CliCommand::Store(command) => command.run(|| config().map(|m| &*m))?,
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
