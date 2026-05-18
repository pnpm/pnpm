pub mod add;
pub mod install;
pub mod run;
pub mod store;
pub mod supported_architectures;

use crate::State;
use add::AddArgs;
use clap::{Parser, Subcommand, ValueEnum};
use install::InstallArgs;
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Config, Host};
use pacquet_executor::execute_shell;
use pacquet_package_manifest::PackageManifest;
use pacquet_reporter::{NdjsonReporter, SilentReporter};
use run::RunArgs;
use std::path::PathBuf;
use store::StoreCommand;

/// Experimental package manager for node.js written in rust.
#[derive(Debug, Parser)]
#[clap(name = "pacquet")]
#[clap(bin_name = "pacquet")]
#[clap(version = "0.2.1")]
#[clap(about = "Experimental package manager for node.js")]
pub struct CliArgs {
    #[clap(subcommand)]
    pub command: CliCommand,

    /// Set working directory.
    #[clap(short = 'C', long, default_value = ".")]
    pub dir: PathBuf,

    /// Reporter output format.
    #[clap(long, value_enum, default_value_t = ReporterType::Silent, global = true)]
    pub reporter: ReporterType,
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

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Initialize a package.json
    Init,
    /// Add a package
    Add(AddArgs),
    /// Install packages
    Install(InstallArgs),
    /// Runs a package's "test" script, if one was provided.
    Test,
    /// Runs a defined package script.
    Run(RunArgs),
    /// Runs an arbitrary command specified in the package's start property of its scripts object.
    Start,
    /// Managing the package store.
    #[clap(subcommand)]
    Store(StoreCommand),
}

impl CliArgs {
    /// Execute the command
    pub async fn run(self) -> miette::Result<()> {
        let CliArgs { command, dir, reporter } = self;
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
            Config::current::<Host, _, _, _, _>(
                || Ok::<_, std::convert::Infallible>(dir.clone()),
                home::home_dir,
                Default::default,
            )
            .map(Config::leak)
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
                let require_lockfile = args.frozen_lockfile;
                let state = State::init(manifest_path(), cfg, require_lockfile)
                    .wrap_err("initialize the state")?;
                match reporter {
                    ReporterType::Ndjson => args.run::<NdjsonReporter>(state).await?,
                    ReporterType::Silent => args.run::<SilentReporter>(state).await?,
                }
            }
            CliCommand::Test => {
                let manifest = PackageManifest::from_path(manifest_path())
                    .wrap_err("getting the package.json in current directory")?;
                if let Some(script) = manifest.script("test", false)? {
                    execute_shell(script)
                        .wrap_err(format!("executing command: \"{0}\"", script))?;
                }
            }
            CliCommand::Run(args) => args.run(manifest_path())?,
            CliCommand::Start => {
                // Runs an arbitrary command specified in the package's start property of its scripts
                // object. If no start property is specified on the scripts object, it will attempt to
                // run node server.js as a default, failing if neither are present.
                // The intended usage of the property is to specify a command that starts your program.
                let manifest = PackageManifest::from_path(manifest_path())
                    .wrap_err("getting the package.json in current directory")?;
                let command = manifest.script("start", true)?.unwrap_or("node server.js");
                execute_shell(command).wrap_err(format!("executing command: \"{0}\"", command))?;
            }
            CliCommand::Store(command) => command.run(|| config().map(|m| &*m))?,
        }

        Ok(())
    }
}
